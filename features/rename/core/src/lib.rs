#![forbid(unsafe_code)]

mod kind;
mod many;
mod undo;

use std::ffi::OsStr;
use std::fs::{DirBuilder, OpenOptions};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::Path;

use nova_files_api::{Failure, Outcome};

use nova_files_jobs::Ended;
use nova_files_jobs::failure::failure;
use nova_files_jobs::place::rename_no_replace;
use nova_files_rename_api::is_plain_name;

pub use kind::Renames;
pub use many::rename_many;

const FOLDER_MODE: u32 = 0o777;
const FILE_MODE: u32 = 0o666;

pub fn rename(path: &Path, name: &OsStr) -> Ended {
    let (Some(folder), Some(old_name)) = (path.parent(), path.file_name()) else {
        return failed(path, Failure::InvalidName);
    };
    if !path.is_absolute() || !is_plain_name(name) {
        return failed(path, Failure::InvalidName);
    }
    if old_name == name {
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

pub fn new_file(inside: &Path, name: &OsStr) -> Ended {
    let path = inside.join(name);
    if !inside.is_absolute() || !is_plain_name(name) {
        return failed(&path, Failure::InvalidName);
    }
    let made = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(FILE_MODE)
        .open(&path);
    match made {
        Ok(_) => (Outcome::Done, vec![inside.to_path_buf()]),
        Err(error) => failed(&path, failure(&error)),
    }
}

fn failed(path: &Path, reason: Failure) -> Ended {
    let path = path.to_path_buf();
    (Outcome::Failed { path, reason }, Vec::new())
}
