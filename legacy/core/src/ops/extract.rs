use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use nova_files_api::{Conflict, ConflictKind, ExtractTo, Failure, Outcome};

use super::Ended;
use super::journal::word;
use super::make::How;
use super::plan::pairs_into;
use super::running::Running;
use super::stop::Stop;
use super::transfer::transfer_to;
use crate::archive::{ArchiveEntry, ArchiveFailure, ArchiveKind, Archiver, Extracting, Flow};

pub const STAGING_PREFIX: &str = ".nova-extract-";
const ENDINGS: [&str; 13] = [
    ".tar.gz", ".tar.xz", ".tar.zst", ".tar.bz2", ".tgz", ".txz", ".tzst", ".tbz2", ".tbz", ".tar",
    ".zip", ".7z", ".rar",
];
const OWNER_WRITE: u32 = 0o200;
const OWNER_ALL: u32 = 0o700;

#[derive(Debug, Clone, Copy)]
pub enum Taking<'a> {
    All(ExtractTo),
    Picked(&'a [PathBuf]),
}

pub fn extract(
    running: &mut Running,
    archiver: Option<&Archiver>,
    archive: &Path,
    into: &Path,
    taking: Taking<'_>,
) -> Ended {
    if !archive.is_absolute() || archive.file_name().is_none() {
        return (
            Stop::because(archive, Failure::InvalidName).outcome(),
            Vec::new(),
        );
    }
    if !into.is_absolute() {
        return (
            Stop::because(into, Failure::InvalidName).outcome(),
            Vec::new(),
        );
    }
    let Some(archiver) = archiver else {
        return (Stop::because(archive, Failure::Other).outcome(), Vec::new());
    };
    let staging = match unpack(running, archiver, (archive, taking), into) {
        Ok(Some(staging)) => staging,
        Ok(None) => return (Outcome::Cancelled, Vec::new()),
        Err(stop) => return (stop.outcome(), Vec::new()),
    };
    let placed = place(running, &staging, into, taking);
    clear(&staging.root);
    match placed {
        Ok(changed) => (Outcome::Done, changed),
        Err((stop, changed)) => (stop.outcome(), changed),
    }
}

struct Staging {
    root: PathBuf,
    named: PathBuf,
}

fn unpack(
    running: &mut Running,
    archiver: &Archiver,
    (archive, taking): (&Path, Taking<'_>),
    into: &Path,
) -> Result<Option<Staging>, Stop> {
    let Some((mut entries, mut password)) = listed(running, archiver, archive, into)? else {
        return Ok(None);
    };
    let picked = match taking {
        Taking::All(_) => &[][..],
        Taking::Picked(picked) => picked,
    };
    if !picked.is_empty() {
        entries.retain(|entry| picked.iter().any(|path| entry.path.starts_with(path)));
    }
    let (bytes, items) = totals(&entries);
    running.reporter.planned(bytes, items);
    if password.is_none() && entries.iter().any(|entry| entry.locked) {
        let Some(given) = ask_password(running, ConflictKind::PasswordNeeded, archive, into)?
        else {
            return Ok(None);
        };
        password = Some(given);
    }
    for attempt in 0_u64.. {
        let staging = staging(into, running.names, attempt, &folder_name(archive))?;
        let unpacked = archiver.extract_picked(
            (archive, picked),
            &staging.named,
            password.as_deref(),
            |told| steer(running, told),
        );
        let kind = match unpacked {
            Ok(()) => return Ok(Some(staging)),
            Err(ArchiveFailure::NeedsPassword) => ConflictKind::PasswordNeeded,
            Err(ArchiveFailure::WrongPassword) => ConflictKind::PasswordWrong,
            Err(failure) => {
                clear(&staging.root);
                return Err(stop_for(failure, archive, into));
            }
        };
        clear(&staging.root);
        running.reporter.start_over();
        let Some(given) = ask_password(running, kind, archive, into)? else {
            return Ok(None);
        };
        password = Some(given);
    }
    Err(Stop::because(archive, Failure::Other))
}

type Listed = (Vec<ArchiveEntry>, Option<String>);

fn listed(
    running: &mut Running,
    archiver: &Archiver,
    archive: &Path,
    into: &Path,
) -> Result<Option<Listed>, Stop> {
    let mut password = None;
    loop {
        let kind = match archiver.list_locked(archive, password.as_deref()) {
            Ok(entries) => return Ok(Some((entries, password))),
            Err(ArchiveFailure::NeedsPassword) => ConflictKind::PasswordNeeded,
            Err(ArchiveFailure::WrongPassword) => ConflictKind::PasswordWrong,
            Err(failure) => return Err(stop_for(failure, archive, into)),
        };
        let Some(given) = ask_password(running, kind, archive, into)? else {
            return Ok(None);
        };
        password = Some(given);
    }
}

pub fn ask_password(
    running: &mut Running,
    kind: ConflictKind,
    archive: &Path,
    into: &Path,
) -> Result<Option<String>, Stop> {
    let conflict = Conflict {
        kind,
        source: archive.to_path_buf(),
        target: into.to_path_buf(),
        number: 0,
        more: 0,
    };
    if running.skips(conflict)? {
        return Ok(None);
    }
    Ok(Some(
        running.shared.control.take_password().unwrap_or_default(),
    ))
}

pub fn steer(running: &mut Running, told: Extracting) -> Flow {
    let control = &running.shared.control;
    match told {
        Extracting::Entry(bytes) => running.reporter.item(bytes),
        Extracting::Waiting => {}
        Extracting::Held => {
            return if control.checkpoint().is_ok() {
                Flow::Go
            } else {
                Flow::Stop
            };
        }
    }
    if control.cancelled() {
        Flow::Stop
    } else if control.paused() {
        Flow::Hold
    } else {
        Flow::Go
    }
}

fn staging(into: &Path, id: u64, attempt: u64, folder: &OsStr) -> Result<Staging, Stop> {
    let root = into.join(format!("{STAGING_PREFIX}{id}-{attempt}"));
    fs::create_dir(&root).map_err(|error| Stop::at(into, &error))?;
    let named = root.join(folder);
    if let Err(error) = fs::create_dir(&named) {
        clear(&root);
        return Err(Stop::at(into, &error));
    }
    Ok(Staging { root, named })
}

fn place(
    running: &mut Running,
    staging: &Staging,
    into: &Path,
    taking: Taking<'_>,
) -> Result<Vec<PathBuf>, (Stop, Vec<PathBuf>)> {
    let unpacked = top(&staging.named).map_err(|error| (Stop::at(into, &error), Vec::new()))?;
    let sources = match taking {
        Taking::All(ExtractTo::Here) if unpacked.len() > 1 => vec![staging.named.clone()],
        Taking::All(ExtractTo::Here | ExtractTo::Inside) => unpacked,
        Taking::Picked(picked) => picked
            .iter()
            .map(|path| staging.named.join(path))
            .filter(|path| path.symlink_metadata().is_ok())
            .collect(),
    };
    if sources.is_empty() {
        return Ok(Vec::new());
    }
    let locked = writable(&sources);
    running.record_job(word::MOVE, Some(into), &sources);
    running.reporter.start_over();
    let (result, touched) = transfer_to(running, &pairs_into(&sources, into), How::Move);
    for (source, mode) in locked {
        let target = running.steps.get(&source).cloned();
        if let Some(target) = target {
            fs::set_permissions(target, fs::Permissions::from_mode(mode)).ok();
        }
    }
    let changed = if touched.is_empty() {
        Vec::new()
    } else {
        vec![into.to_path_buf()]
    };
    result
        .map(|()| changed.clone())
        .map_err(|stop| (stop, changed))
}

fn top(folder: &Path) -> io::Result<Vec<PathBuf>> {
    fs::read_dir(folder)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect()
}

fn writable(sources: &[PathBuf]) -> Vec<(PathBuf, u32)> {
    sources
        .iter()
        .filter_map(|source| {
            let metadata = source
                .symlink_metadata()
                .ok()
                .filter(fs::Metadata::is_dir)?;
            let mode = metadata.permissions().mode();
            fs::set_permissions(source, fs::Permissions::from_mode(mode | OWNER_WRITE)).ok()?;
            Some((source.clone(), mode))
        })
        .collect()
}

fn clear(path: &Path) {
    let Ok(metadata) = path.symlink_metadata() else {
        return;
    };
    if !metadata.is_dir() {
        fs::remove_file(path).ok();
        return;
    }
    let mode = metadata.permissions().mode();
    fs::set_permissions(path, fs::Permissions::from_mode(mode | OWNER_ALL)).ok();
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            clear(&entry.path());
        }
    }
    fs::remove_dir(path).ok();
}

fn totals(entries: &[ArchiveEntry]) -> (u64, u64) {
    let bytes = entries
        .iter()
        .filter(|entry| entry.kind == ArchiveKind::File)
        .map(|entry| entry.size)
        .fold(0_u64, u64::saturating_add);
    (bytes, u64::try_from(entries.len()).unwrap_or(u64::MAX))
}

fn folder_name(archive: &Path) -> OsString {
    let name = archive.file_name().unwrap_or_default();
    let Some(text) = name.to_str() else {
        return archive.file_stem().unwrap_or(name).to_os_string();
    };
    let lower = text.to_lowercase();
    let cut = ENDINGS
        .iter()
        .find(|ending| lower.ends_with(*ending) && lower.len() > ending.len())
        .map_or(text.len(), |ending| text.len() - ending.len());
    text.get(..cut)
        .map_or_else(|| name.to_os_string(), Into::into)
}

fn stop_for(failure: ArchiveFailure, archive: &Path, into: &Path) -> Stop {
    let (path, reason) = match failure {
        ArchiveFailure::Cancelled => return Stop::Cancelled,
        ArchiveFailure::Gone => (archive, Failure::NotFound),
        ArchiveFailure::Denied => (archive, Failure::PermissionDenied),
        ArchiveFailure::Broken | ArchiveFailure::NeedsPassword | ArchiveFailure::WrongPassword => {
            (archive, Failure::Damaged)
        }
        ArchiveFailure::Unsupported => (archive, Failure::NotArchive),
        ArchiveFailure::NotRead => (archive, Failure::Other),
        ArchiveFailure::NoSpace => (into, Failure::NoSpace),
        ArchiveFailure::ReadOnly => (into, Failure::ReadOnly),
        ArchiveFailure::NotWritten => (into, Failure::Other),
    };
    Stop::because(path, reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_folder_after_the_archive() {
        let name = |archive: &str| folder_name(Path::new(archive));
        assert_eq!(name("/a/photos.zip"), "photos");
        assert_eq!(name("/a/photos.ZIP"), "photos");
        assert_eq!(name("/a/src-1.2.tar.gz"), "src-1.2");
        assert_eq!(name("/a/src.tar.zst"), "src");
        assert_eq!(name("/a/src.tgz"), "src");
        assert_eq!(name("/a/notes.txt"), "notes.txt");
        assert_eq!(name("/a/.zip"), ".zip");
        assert_eq!(name("/a/x.7z"), "x");
    }

    #[test]
    fn totals_count_file_bytes_and_every_entry() {
        let entry = |kind, size| ArchiveEntry {
            kind,
            locked: false,
            size,
            modified: 0,
            mode: 0o644,
            path: "a".into(),
            link: PathBuf::new(),
        };
        let entries = [
            entry(ArchiveKind::File, 5),
            entry(ArchiveKind::Folder, 7),
            entry(ArchiveKind::File, 3),
        ];
        assert_eq!(totals(&entries), (8, 3));
        assert_eq!(totals(&[]), (0, 0));
    }

    #[test]
    fn says_where_and_why_it_stopped() {
        let (archive, into) = (Path::new("/a/x.zip"), Path::new("/b"));
        let stop = |failure| stop_for(failure, archive, into);
        assert_eq!(stop(ArchiveFailure::Cancelled), Stop::Cancelled);
        assert_eq!(
            stop(ArchiveFailure::Broken),
            Stop::because(archive, Failure::Damaged)
        );
        assert_eq!(
            stop(ArchiveFailure::Unsupported),
            Stop::because(archive, Failure::NotArchive)
        );
        assert_eq!(
            stop(ArchiveFailure::Gone),
            Stop::because(archive, Failure::NotFound)
        );
        assert_eq!(
            stop(ArchiveFailure::Denied),
            Stop::because(archive, Failure::PermissionDenied)
        );
        assert_eq!(
            stop(ArchiveFailure::NoSpace),
            Stop::because(into, Failure::NoSpace)
        );
        assert_eq!(
            stop(ArchiveFailure::ReadOnly),
            Stop::because(into, Failure::ReadOnly)
        );
        assert_eq!(
            stop(ArchiveFailure::NotWritten),
            Stop::because(into, Failure::Other)
        );
    }
}
