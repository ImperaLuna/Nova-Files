use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::{OsStr, OsString};
use std::fs::OpenOptions;
use std::io::{self, Write as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use nova_files_api::{Conflict, ConflictKind, Failure, Outcome};

use super::Ended;
use super::clash::conflict_for;
use super::failure::failure;
use super::journal::{PART_PREFIX, Record, word};
use super::place::rename_no_replace;
use super::plan::Entry;
use super::running::Running;
use super::stop::Stop;
use super::sync::{remove, sync_folder, sync_folders};
use crate::private_file::FILE_MODE;
use crate::trash::{
    FILES, INFO_SUFFIX, TrashFolder, add_folder_sizes, folder_for, folder_size, info_text,
    stored_name,
};

type Known = HashMap<PathBuf, TrashFolder>;

const MAX_ATTEMPTS: u32 = 10_000;

enum Refused {
    NoTrash,
    Failed(Failure),
}

struct Trashed {
    trash: PathBuf,
    stored: OsString,
    info: PathBuf,
}

pub fn trash(running: &mut Running, paths: &[PathBuf], home_trash: Option<&Path>) -> Ended {
    running.record_job(word::TRASH, None, paths);
    if let Err(stop) = check(paths) {
        return (stop.outcome(), Vec::new());
    }
    let items = u64::try_from(paths.len()).unwrap_or(u64::MAX);
    running.reporter.planned(0, items);
    let mut touched = BTreeSet::new();
    let mut folders = Vec::new();
    let mut result = Ok(());
    let mut ordered: Vec<_> = paths.iter().collect();
    ordered.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    let mut known = Known::new();
    for (index, path) in ordered.into_iter().enumerate() {
        if !running.before.trashed.contains(path) {
            let at = (index, path.as_path());
            match trash_one(running, at, (home_trash, &mut known), &mut touched) {
                Ok(folder) => folders.extend(folder),
                Err(stop) => {
                    result = Err(stop);
                    break;
                }
            }
        }
        running.reporter.item(0);
    }
    sync_folders(touched.iter().map(PathBuf::as_path));
    note_sizes(running, folders);
    running.reporter.tell();
    (
        result.map_or_else(Stop::outcome, |()| Outcome::Done),
        touched.into_iter().collect(),
    )
}

pub enum SetAside {
    Trashed(PathBuf),
    Gone,
    NoTrash,
    Refused,
}

pub fn trash_set_aside(
    running: &mut Running,
    (index, left): (usize, &Path),
    shown: &Path,
) -> SetAside {
    let home_trash = running.home_trash.clone();
    let mut known = std::mem::take(&mut running.replaced_trashes);
    let at = (index, left, shown.file_name());
    let sent = send(running, at, (home_trash.as_deref(), &mut known));
    running.replaced_trashes = known;
    match sent {
        Ok(Some((_, item, _))) => SetAside::Trashed(item),
        Ok(None) => SetAside::Gone,
        Err(Refused::NoTrash) => SetAside::NoTrash,
        Err(Refused::Failed(reason)) => {
            log::warn!(
                "can't set {} aside in the trash: {reason:?}",
                left.display()
            );
            SetAside::Refused
        }
    }
}

fn check(paths: &[PathBuf]) -> Result<(), Stop> {
    for path in paths {
        if !path.is_absolute() || path.file_name().is_none() {
            return Err(Stop::because(path, Failure::InvalidName));
        }
        path.symlink_metadata()
            .map_err(|error| Stop::at(path, &error))?;
    }
    Ok(())
}

fn trash_one(
    running: &mut Running,
    (index, path): (usize, &Path),
    (home_trash, known): (Option<&Path>, &mut Known),
    touched: &mut BTreeSet<PathBuf>,
) -> Result<Option<Trashed>, Stop> {
    loop {
        running.shared.control.checkpoint()?;
        let refused = match send(running, (index, path, None), (home_trash, known)) {
            Ok(Some((trash, _, folder))) => {
                touched.extend(path.parent().map(Path::to_path_buf));
                touched.insert(trash.files());
                touched.insert(trash.info());
                return Ok(folder);
            }
            Ok(None) => return Ok(None),
            Err(refused) => {
                known.clear();
                refused
            }
        };
        let kind = match refused {
            Refused::NoTrash => ConflictKind::NoTrash,
            Refused::Failed(reason) => conflict_for(reason, Path::new(""), path)
                .ok_or_else(|| Stop::because(path, reason))?,
        };
        let conflict = Conflict {
            kind,
            source: path.to_path_buf(),
            target: path.to_path_buf(),
            number: 0,
            more: 0,
        };
        if running.skips(conflict)? {
            return Ok(None);
        }
    }
}

type Sent = Option<(TrashFolder, PathBuf, Option<Trashed>)>;

fn send(
    running: &mut Running,
    (index, path, shown): (usize, &Path, Option<&OsStr>),
    (home_trash, known): (Option<&Path>, &mut Known),
) -> Result<Sent, Refused> {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(Refused::Failed(Failure::InvalidName));
    };
    let shown = shown.unwrap_or(name);
    let found = parent.canonicalize().and_then(|parent| {
        let at = parent.join(name);
        let there = at.symlink_metadata()?;
        Ok((parent, at, there.is_dir(), Entry::of(&there)))
    });
    let (parent, at, is_folder, item) = match found {
        Ok(found) => found,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(Refused::Failed(failure(&error))),
    };
    let original = parent.join(shown);
    let trash = if let Some(trash) = known.get(&parent) {
        trash.clone()
    } else {
        let trash = folder_for(&parent, home_trash).map_err(|error| no_trash(&original, &error))?;
        known.insert(parent, trash.clone());
        trash
    };
    if original.starts_with(&trash.root) {
        return Err(Refused::NoTrash);
    }
    if trash.root.starts_with(&original) {
        return Err(Refused::Failed(Failure::Unsupported));
    }
    let text = info_text(trash.path_field(&original), SystemTime::now());
    let part = trash
        .info()
        .join(format!("{PART_PREFIX}{}-{index}", running.names));
    let entry = write_part(running, &part, &text).map_err(|error| {
        remove(&part);
        refused(&error)
    })?;
    let moved = take_names(running, (&part, entry), &trash, (&at, shown, item));
    remove(&part);
    match moved {
        Ok(stored) => {
            let item = trash.files().join(&stored);
            running.record(Record::Trashed(path, &item));
            let folder = is_folder.then(|| Trashed {
                trash: trash.root.clone(),
                info: info_path(&trash, &stored),
                stored,
            });
            Ok(Some((trash, item, folder)))
        }
        Err(error) if gone_meanwhile(&error, &at) => Ok(None),
        Err(error) => Err(refused(&error)),
    }
}

fn gone_meanwhile(error: &io::Error, original: &Path) -> bool {
    error.kind() == io::ErrorKind::NotFound && original.symlink_metadata().is_err()
}

fn take_names(
    running: &mut Running,
    (part, entry): (&Path, Entry),
    trash: &TrashFolder,
    (original, name, item): (&Path, &OsStr, Entry),
) -> io::Result<OsString> {
    for attempt in 1..=MAX_ATTEMPTS {
        let stored = stored_name(name, attempt);
        let info = info_path(trash, &stored);
        let target = trash.files().join(&stored);
        if info.symlink_metadata().is_ok() {
            continue;
        }
        running.record(Record::TrashInfo(&info, entry, item));
        running.settle();
        match rename_no_replace(part, &info) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            taken => taken?,
        }
        let moved = sync_folder(&trash.info()).and_then(|()| rename_no_replace(original, &target));
        match moved {
            Ok(()) => return Ok(stored),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                rename_no_replace(&info, part).inspect_err(|_| remove(&info))?;
            }
            Err(error) => {
                remove(&info);
                return Err(error);
            }
        }
    }
    Err(io::ErrorKind::AlreadyExists.into())
}

fn info_path(trash: &TrashFolder, stored: &OsStr) -> PathBuf {
    let mut name = stored.to_os_string();
    name.push(INFO_SUFFIX);
    trash.info().join(name)
}

fn no_trash(original: &Path, error: &io::Error) -> Refused {
    log::info!("{} has no trash: {error}", original.display());
    match failure(error) {
        reason @ (Failure::NoSpace | Failure::ReadOnly) => Refused::Failed(reason),
        _ => Refused::NoTrash,
    }
}

fn refused(error: &io::Error) -> Refused {
    let crossing = [rustix::io::Errno::XDEV, rustix::io::Errno::BUSY]
        .iter()
        .any(|errno| error.raw_os_error() == Some(errno.raw_os_error()));
    if crossing {
        Refused::Failed(Failure::Unsupported)
    } else {
        Refused::Failed(failure(error))
    }
}

fn write_part(running: &mut Running, part: &Path, text: &[u8]) -> io::Result<Entry> {
    running.record(Record::Part(part));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(FILE_MODE)
        .open(part)?;
    file.write_all(text)?;
    file.sync_data()?;
    Ok(Entry::of(&file.metadata()?))
}

fn note_sizes(running: &Running, folders: Vec<Trashed>) {
    let control = &running.shared.control;
    let go_on = || control.checkpoint().is_ok();
    let mut sized: BTreeMap<PathBuf, Vec<(Trashed, u64)>> = BTreeMap::new();
    for folder in folders {
        let path = folder.trash.join(FILES).join(&folder.stored);
        match folder_size(&path, &go_on) {
            Ok(Some(size)) => sized
                .entry(folder.trash.clone())
                .or_default()
                .push((folder, size)),
            Ok(None) => return,
            Err(error) => log::info!("{} has no size: {error}", path.display()),
        }
    }
    for (trash, folders) in &sized {
        let lines: Vec<_> = folders
            .iter()
            .map(|(folder, size)| (folder.stored.as_os_str(), folder.info.as_path(), *size))
            .collect();
        if let Err(error) = add_folder_sizes(trash, &lines) {
            log::info!("{} keeps no sizes: {error}", trash.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustix::io::Errno;

    fn os_error(errno: Errno) -> io::Error {
        io::Error::from_raw_os_error(errno.raw_os_error())
    }

    #[test]
    fn a_full_drive_is_the_users_to_free_not_a_missing_trash() {
        let path = Path::new("/a");

        assert!(matches!(
            no_trash(path, &os_error(Errno::NOSPC)),
            Refused::Failed(Failure::NoSpace)
        ));
        assert!(matches!(
            no_trash(path, &os_error(Errno::ACCESS)),
            Refused::NoTrash
        ));
        assert!(matches!(
            no_trash(path, &os_error(Errno::ROFS)),
            Refused::Failed(Failure::ReadOnly)
        ));
    }

    #[test]
    fn a_move_refused_across_drives_or_as_busy_is_unsupported() {
        for errno in [Errno::XDEV, Errno::BUSY] {
            assert!(matches!(
                refused(&os_error(errno)),
                Refused::Failed(Failure::Unsupported)
            ));
        }
        assert!(matches!(
            refused(&os_error(Errno::ACCESS)),
            Refused::Failed(Failure::PermissionDenied)
        ));
    }
}
