use std::fs;
use std::io;
use std::path::Path;

pub use crate::private_file::sync_folder;

pub fn sync_folders<'a>(folders: impl IntoIterator<Item = &'a Path>) {
    for folder in folders {
        if let Err(error) = sync_folder(folder) {
            log::warn!("can't sync the folder {}: {error}", folder.display());
        }
    }
}

pub fn remove(path: &Path) {
    if let Err(error) = fs::remove_file(path)
        && error.kind() != io::ErrorKind::NotFound
    {
        log::warn!("can't remove {}: {error}", path.display());
    }
}
