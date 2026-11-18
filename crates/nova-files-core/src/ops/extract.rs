//! Extracting an archive. The helper unpacks it into a hidden folder in
//! the target folder (see `archive`), then what it unpacked moves into
//! place as a move does, asking about names already taken; the journal
//! records that move, so a crash resumes it as one. A locked archive asks
//! for its password, again each time it is wrong.

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

/// The start of the hidden folder an archive unpacks into, followed by
/// the job's id for names and the try.
pub const STAGING_PREFIX: &str = ".nova-extract-";
/// The endings an archive's name loses to name the folder it extracts
/// to, longest first where one ends another.
const ENDINGS: [&str; 13] = [
    ".tar.gz", ".tar.xz", ".tar.zst", ".tar.bz2", ".tgz", ".txz", ".tzst", ".tbz2", ".tbz", ".tar",
    ".zip", ".7z", ".rar",
];
const OWNER_WRITE: u32 = 0o200;
const OWNER_ALL: u32 = 0o700;

/// Extracts `archive` into `into` as `to` says, with `archiver`.
pub fn extract(
    running: &mut Running,
    archiver: Option<&Archiver>,
    archive: &Path,
    into: &Path,
    to: ExtractTo,
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
    let staging = match unpack(running, archiver, archive, into) {
        Ok(Some(staging)) => staging,
        Ok(None) => return (Outcome::Cancelled, Vec::new()),
        Err(stop) => return (stop.outcome(), Vec::new()),
    };
    let placed = place(running, &staging, into, to);
    clear(&staging.root);
    match placed {
        Ok(changed) => (Outcome::Done, changed),
        Err((stop, changed)) => (stop.outcome(), changed),
    }
}

/// The hidden folder an archive unpacked into, and the folder inside it
/// named after the archive, which holds what it unpacked.
struct Staging {
    root: PathBuf,
    named: PathBuf,
}

/// Unpacks `archive` into a hidden folder in `into`, asking for its
/// password while one is needed; `None` when the user skips it.
fn unpack(
    running: &mut Running,
    archiver: &Archiver,
    archive: &Path,
    into: &Path,
) -> Result<Option<Staging>, Stop> {
    let entries = archiver
        .list(archive)
        .map_err(|failure| stop_for(failure, archive, into))?;
    let (bytes, items) = totals(&entries);
    running.reporter.planned(bytes, items);
    let mut password = if entries.iter().any(|entry| entry.locked) {
        let Some(given) = ask_password(running, ConflictKind::PasswordNeeded, archive, into)?
        else {
            return Ok(None);
        };
        Some(given)
    } else {
        None
    };
    for attempt in 0_u64.. {
        let staging = staging(into, running.names, attempt, &folder_name(archive))?;
        let unpacked = archiver.extract(archive, &staging.named, password.as_deref(), |told| {
            steer(running, told)
        });
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

/// Asks for the password of `archive` with a conflict of `kind`; `None`
/// when the user skips it.
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
        // Numbered when asked.
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

/// What the helper should do next, having told `told`.
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

/// Makes the hidden folder in `into` for try `attempt` of the job whose
/// names take `id`, and the folder `folder` inside it.
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

/// Moves what `staging` holds into `into` as `to` says, and says which
/// folders changed.
fn place(
    running: &mut Running,
    staging: &Staging,
    into: &Path,
    to: ExtractTo,
) -> Result<Vec<PathBuf>, (Stop, Vec<PathBuf>)> {
    let unpacked = top(&staging.named).map_err(|error| (Stop::at(into, &error), Vec::new()))?;
    let sources = match to {
        ExtractTo::Here if unpacked.len() > 1 => vec![staging.named.clone()],
        ExtractTo::Here | ExtractTo::Inside => unpacked,
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
        // Put back on what moved whole; a merged folder stays as it was.
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

/// The entries at the top of `folder`.
fn top(folder: &Path) -> io::Result<Vec<PathBuf>> {
    fs::read_dir(folder)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect()
}

/// Lets the owner write in each folder of `sources`, as a folder that
/// moves to another folder must be writable; says what each had before.
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

/// Removes `path` and all inside it, letting the owner into each folder
/// first, as an archive's folders may forbid it.
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

/// The bytes and items `entries` hold.
fn totals(entries: &[ArchiveEntry]) -> (u64, u64) {
    let bytes = entries
        .iter()
        .filter(|entry| entry.kind == ArchiveKind::File)
        .map(|entry| entry.size)
        .fold(0_u64, u64::saturating_add);
    (bytes, u64::try_from(entries.len()).unwrap_or(u64::MAX))
}

/// The name of the folder `archive` extracts to: its name without the
/// archive's ending.
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

/// Why extracting `archive` into `into` stopped, from the helper's
/// `failure`.
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
