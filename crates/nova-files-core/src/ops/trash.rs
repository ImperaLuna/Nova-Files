//! The trash job: each entry goes to the trash on its own mount (see
//! `crate::trash`), one rename each, never a copy.
//!
//! For each entry its info file is first written whole under a part
//! file's hidden name in the trash's `info/` and synced; then, recorded
//! in the journal with its device and inode, it takes its real name,
//! never over anything, and that folder is synced; only then is the entry
//! renamed into the trash's `files/` under the same name, never over
//! anything, and recorded as the path the user gave. A name another
//! app's entry holds in `files/` sends the info file back to try the next
//! name. After a crash, the part file goes as any part
//! file does, and an info file still the job's own whose entry never got
//! there goes too (see `left`), so a crash leaves neither half an info
//! file nor one naming nothing. The trashed folders' sizes go in their
//! trash's cache once every entry has gone.
//!
//! A mount with no trash the user may use, or an entry already in a
//! trash, is a conflict (skip, or retry); so are a full drive and a
//! denied permission. An entry gone since the job started is taken as
//! gone where the user wanted: inside a folder trashed before it, say.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
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
use super::sync::{sync_folder, sync_folders};
use crate::private_file::FILE_MODE;
use crate::trash::{
    FILES, INFO_SUFFIX, TrashFolder, add_folder_sizes, folder_for, folder_size, info_text,
    stored_name,
};

/// How many names an entry tries in the trash before the job gives up
/// on it: something keeps taking them.
const MAX_ATTEMPTS: u32 = 10_000;

/// Why an entry didn't go to the trash.
enum Refused {
    /// Its mount has no trash it may use.
    NoTrash,
    /// A step failed for this reason.
    Failed(Failure),
}

/// A folder the job trashed, whose size its trash's cache is to hold.
struct Trashed {
    trash: PathBuf,
    stored: OsString,
    info: PathBuf,
}

/// Moves each of `paths` to its trash; `home_trash` is the home trash's
/// folder.
pub fn trash(running: &mut Running, paths: &[PathBuf], home_trash: Option<&Path>) -> Ended {
    running.record(Record::Operation(word::TRASH));
    for path in paths {
        running.record(Record::Source(path));
    }
    running.record(Record::SourcesDone);
    if let Err(stop) = check(paths) {
        return (stop.outcome(), Vec::new());
    }
    let items = u64::try_from(paths.len()).unwrap_or(u64::MAX);
    running.reporter.planned(0, items);
    let mut touched = BTreeSet::new();
    let mut folders = Vec::new();
    let mut result = Ok(());
    for (index, path) in paths.iter().enumerate() {
        // Trashed before a crash: what has its name now came since.
        if !running.before.trashed.contains(path) {
            match trash_one(running, (index, path), home_trash, &mut touched) {
                Ok(folder) => folders.extend(folder),
                Err(stop) => {
                    result = Err(stop);
                    break;
                }
            }
        }
        running.reporter.item(0);
    }
    // So what the job reports trashed is on the disk.
    sync_folders(touched.iter().map(PathBuf::as_path));
    note_sizes(running, folders);
    running.reporter.tell();
    (
        result.map_or_else(Stop::outcome, |()| Outcome::Done),
        touched.into_iter().collect(),
    )
}

/// Checks every path before anything goes, so a bad one fails the job
/// with nothing done.
///
/// # Errors
/// [`Stop`] when a path isn't absolute, has no name, or isn't there.
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

/// Moves `path`, the job's `index`th, to its trash, asking what to do
/// when it can't, and notes in `touched` the folders whose entries
/// changed. Returns the folder trashed, if it was one.
///
/// # Errors
/// [`Stop`] when cancelled, or when a failure isn't the user's to answer.
fn trash_one(
    running: &mut Running,
    (index, path): (usize, &Path),
    home_trash: Option<&Path>,
    touched: &mut BTreeSet<PathBuf>,
) -> Result<Option<Trashed>, Stop> {
    loop {
        running.shared.control.checkpoint()?;
        let refused = match send(running, (index, path), home_trash) {
            Ok(Some((trash, folder))) => {
                touched.extend(path.parent().map(Path::to_path_buf));
                touched.insert(trash.files());
                touched.insert(trash.info());
                return Ok(folder);
            }
            Ok(None) => return Ok(None),
            Err(refused) => refused,
        };
        let kind = match refused {
            Refused::NoTrash => ConflictKind::NoTrash,
            // Gone is taken as done above, so not about the source.
            Refused::Failed(reason) => conflict_for(reason, Path::new(""), path)
                .ok_or_else(|| Stop::because(path, reason))?,
        };
        let conflict = Conflict {
            kind,
            source: path.to_path_buf(),
            target: path.to_path_buf(),
            // Numbered when asked.
            number: 0,
        };
        if running.skips(conflict)? {
            return Ok(None);
        }
    }
}

/// What [`send`] did: `None` when the entry was gone already, else its
/// trash and, for a folder, what its size is noted under.
type Sent = Option<(TrashFolder, Option<Trashed>)>;

/// Moves `path`, the job's `index`th, into its trash under a free name,
/// its info file first.
///
/// # Errors
/// [`Refused`] when it stays where it is.
fn send(
    running: &mut Running,
    (index, path): (usize, &Path),
    home_trash: Option<&Path>,
) -> Result<Sent, Refused> {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(Refused::Failed(Failure::InvalidName));
    };
    // Without links, so the trash is the one the entry is really on and
    // the info file says where it really was.
    let found = parent.canonicalize().and_then(|parent| {
        let original = parent.join(name);
        let there = original.symlink_metadata()?;
        Ok((parent, original, there.is_dir(), Entry::of(&there)))
    });
    let (parent, original, is_folder, item) = match found {
        Ok(found) => found,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(Refused::Failed(failure(&error))),
    };
    let trash = folder_for(&parent, home_trash).map_err(|error| no_trash(&original, &error))?;
    // Already in the trash: only a delete takes it further.
    if original.starts_with(&trash.root) {
        return Err(Refused::NoTrash);
    }
    // It holds the trash itself, which can't go into itself.
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
    let moved = take_names(running, (&part, entry), &trash, (&original, name, item));
    remove(&part);
    match moved {
        Ok(stored) => {
            running.record(Record::Trashed(path, &trash.files().join(&stored)));
            let folder = is_folder.then(|| Trashed {
                trash: trash.root.clone(),
                info: info_path(&trash, &stored),
                stored,
            });
            Ok(Some((trash, folder)))
        }
        Err(error) if gone_meanwhile(&error, &original) => Ok(None),
        Err(error) => Err(refused(&error)),
    }
}

/// Whether `error`, from moving `original` to the trash, means it was
/// removed meanwhile.
fn gone_meanwhile(error: &io::Error, original: &Path) -> bool {
    error.kind() == io::ErrorKind::NotFound && original.symlink_metadata().is_err()
}

/// Puts the info file written as `part`, the entry `entry`, and then
/// `original`, named `name` and the entry `item`, into `trash`, each
/// under the first name free for both, and returns that name.
///
/// # Errors
/// When a step fails; the info file is then back at `part`, or gone.
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
        // An info file has it: on to the next name, with nothing to
        // record. A stray entry without one is found below.
        if info.symlink_metadata().is_ok() {
            continue;
        }
        running.record(Record::TrashInfo(&info, entry, item));
        // On the disk before the info file takes its name.
        running.settle();
        match rename_no_replace(part, &info) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            taken => taken?,
        }
        let moved = sync_folder(&trash.info()).and_then(|()| rename_no_replace(original, &target));
        match moved {
            Ok(()) => return Ok(stored),
            // Another app's entry has the name: the info file steps back
            // for the next one.
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

/// Where the info file of the entry stored as `stored` in `trash` goes.
fn info_path(trash: &TrashFolder, stored: &OsStr) -> PathBuf {
    let mut name = stored.to_os_string();
    name.push(INFO_SUFFIX);
    trash.info().join(name)
}

/// Why the trash for `original` couldn't be used or made: a full drive
/// is the user's to free; anything else means there is no trash there.
fn no_trash(original: &Path, error: &io::Error) -> Refused {
    log::info!("{} has no trash: {error}", original.display());
    match failure(error) {
        Failure::NoSpace => Refused::Failed(Failure::NoSpace),
        _ => Refused::NoTrash,
    }
}

/// What a failed step means. The trash is on the entry's own mount and
/// device, so a rename refused as crossing drives, or as busy, means the
/// entry is a mount's top: it can't move at all, and deleting it would
/// empty the drive, so it is never offered as having no trash.
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

/// Writes `text` whole to a new part file at `part`, recorded before it
/// is made, synced, and returns which entry it is.
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

/// Removes the file at `path`, when it is there.
pub(super) fn remove(path: &Path) {
    if let Err(error) = fs::remove_file(path)
        && error.kind() != io::ErrorKind::NotFound
    {
        log::warn!("can't remove {}: {error}", path.display());
    }
}

/// Adds the size of each folder in `folders` to its trash's cache, one
/// write per trash. Only a cache: a failure or a cancel leaves sizes out.
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
            // Cancelled: nothing more is written.
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
