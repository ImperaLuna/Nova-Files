//! The jobs that are a single change: renaming an entry and making a
//! folder. Neither ever replaces what already has the name.

use std::ffi::OsStr;
use std::fs::DirBuilder;
use std::os::unix::fs::DirBuilderExt as _;
use std::path::Path;

use nova_files_api::{Failure, Outcome};

use super::Ended;
use super::failure::failure;
use super::name::is_plain_name;
use super::place::rename_no_replace;

/// A new folder's mode before the user's umask, as `mkdir` gives it.
const FOLDER_MODE: u32 = 0o777;

/// Gives `path` the name `name` in its own folder.
pub fn rename(path: &Path, name: &OsStr) -> Ended {
    let (Some(folder), Some(old_name)) = (path.parent(), path.file_name()) else {
        return failed(path, Failure::InvalidName);
    };
    if !path.is_absolute() || !is_plain_name(name) {
        return failed(path, Failure::InvalidName);
    }
    if old_name == name {
        // Nothing to change, as long as it is still there.
        return match path.symlink_metadata() {
            Ok(_) => (Outcome::Done, Vec::new()),
            Err(error) => failed(path, failure(&error)),
        };
    }
    match rename_no_replace(path, &folder.join(name)) {
        Ok(()) => (Outcome::Done, vec![folder.to_path_buf()]),
        Err(error) => failed(path, failure(&error)),
    }
}

/// Makes an empty folder `name` in `inside`.
pub fn new_folder(inside: &Path, name: &OsStr) -> Ended {
    let path = inside.join(name);
    if !inside.is_absolute() || !is_plain_name(name) {
        return failed(&path, Failure::InvalidName);
    }
    match DirBuilder::new().mode(FOLDER_MODE).create(&path) {
        Ok(()) => (Outcome::Done, vec![inside.to_path_buf()]),
        Err(error) => failed(&path, failure(&error)),
    }
}

fn failed(path: &Path, reason: Failure) -> Ended {
    let path = path.to_path_buf();
    (Outcome::Failed { path, reason }, Vec::new())
}
