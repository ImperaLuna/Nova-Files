//! Syncing folders, so the names a job put in them, or took out, are on
//! the disk and a crash can't lose them; and removing a job's leftovers.

use std::fs::{self, File};
use std::io;
use std::path::Path;

/// Syncs `folder`.
///
/// # Errors
/// When it can't be opened or synced.
pub fn sync_folder(folder: &Path) -> io::Result<()> {
    File::open(folder)?.sync_all()
}

/// Syncs each of `folders` before a job says it is done. A failure means
/// only that a crash right now could lose an entry, so it is logged.
pub fn sync_folders<'a>(folders: impl IntoIterator<Item = &'a Path>) {
    for folder in folders {
        if let Err(error) = sync_folder(folder) {
            log::warn!("can't sync the folder {}: {error}", folder.display());
        }
    }
}

/// Removes the file at `path`, a leftover of a job, when it is there. A
/// failure leaves it, logged.
pub fn remove(path: &Path) {
    if let Err(error) = fs::remove_file(path)
        && error.kind() != io::ErrorKind::NotFound
    {
        log::warn!("can't remove {}: {error}", path.display());
    }
}
