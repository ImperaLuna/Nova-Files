//! Syncing folders, so the names a job put in them, or took out, are on
//! the disk and a crash can't lose them.

use std::fs::File;
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
