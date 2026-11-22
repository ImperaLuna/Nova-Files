use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::{OsStr, OsString};
use std::fs::OpenOptions;
use std::io::{self, Write as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use nova_files_api::{Conflict, ConflictKind, Failure, Outcome};

use crate::restoring::put_back;
use crate::trashes::{
    FILES, INFO_SUFFIX, TrashFolder, Trashes, add_folder_sizes, find, folder_for, folder_size,
    info_text, stored_name,
};
use nova_files_disk::private_file::FILE_MODE;
use nova_files_jobs::Ended;
use nova_files_jobs::clash::conflict_for;
use nova_files_jobs::failure::failure;
use nova_files_jobs::journal::{PART_PREFIX, Record, word};
use nova_files_jobs::mover::Mover;
use nova_files_jobs::place::rename_no_replace;
use nova_files_jobs::plan::Entry;
use nova_files_jobs::running::Running;
use nova_files_jobs::set_aside::{Aside, SetAside, SetAsideForJob};
use nova_files_jobs::stop::Stop;
use nova_files_jobs::sync::{remove, sync_folder, sync_folders};

type Known = HashMap<PathBuf, TrashFolder>;

const MAX_ATTEMPTS: u32 = 10_000;

enum Refused {
    NoTrash,
    InTrash,
    Failed(Failure),
}

struct Trashed {
    trash: PathBuf,
    stored: OsString,
    info: PathBuf,
}

pub fn trash(running: &mut Running, paths: &[PathBuf], home_trash: Option<&Path>) -> Ended {
    running.record_job(word::TRASH, None, paths);
    running.follow(paths);
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

pub struct Trasher {
    trashes: Trashes,
    files: Arc<dyn Mover>,
}

impl Trasher {
    pub fn new(trashes: Trashes, files: Arc<dyn Mover>) -> Self {
        Self { trashes, files }
    }
}

impl SetAside for Trasher {
    fn for_job(&self) -> Box<dyn SetAsideForJob> {
        Box::new(TrashForJob {
            trashes: self.trashes.clone(),
            files: Arc::clone(&self.files),
            known: Known::new(),
        })
    }
}

struct TrashForJob {
    trashes: Trashes,
    files: Arc<dyn Mover>,
    known: Known,
}

impl SetAsideForJob for TrashForJob {
    fn set_aside(
        &mut self,
        running: &mut Running,
        (index, left): (usize, &Path),
        shown: &Path,
    ) -> Aside {
        let at = (index, left, shown.file_name());
        match send(running, at, (self.trashes.home.as_deref(), &mut self.known)) {
            Ok(Some((_, item, _))) => Aside::Kept(item),
            Ok(None) => Aside::Gone,
            Err(Refused::NoTrash | Refused::InTrash) => Aside::Nowhere,
            Err(Refused::Failed(reason)) => {
                log::warn!(
                    "can't set {} aside in the trash: {reason:?}",
                    left.display()
                );
                Aside::Refused
            }
        }
    }

    fn trash(&mut self, running: &mut Running, paths: &[PathBuf]) -> Ended {
        trash(running, paths, self.trashes.home.as_deref())
    }

    fn put_back(&mut self, running: &mut Running, items: &[PathBuf]) -> Option<Ended> {
        let items: Vec<_> = items
            .iter()
            .filter(|item| {
                find(item, &self.trashes)
                    .is_some_and(|(_, found)| found.original.symlink_metadata().is_err())
            })
            .cloned()
            .collect();
        (!items.is_empty()).then(|| put_back(running, &items, &self.trashes, self.files.as_ref()))
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
            Refused::InTrash => ConflictKind::InTrash,
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
        return Err(Refused::InTrash);
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
