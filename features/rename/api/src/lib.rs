//! Renaming and making folders and files: the jobs, and the names a rename or a
//! new entry may take.

#![forbid(unsafe_code)]

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt as _;
use std::path::PathBuf;

use nova_files_api::Operation;

const NUL: u8 = b'\0';
const SEPARATOR: u8 = b'/';

/// Give the file or folder at `path` the name `name`, in the same folder.
///
/// A name already taken fails the job; nothing is replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameJob {
    pub path: PathBuf,
    pub name: OsString,
}

/// Make an empty folder named `name` in the folder `inside`.
///
/// A name already taken fails the job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewFolderJob {
    pub inside: PathBuf,
    pub name: OsString,
}

/// Make an empty file named `name` in the folder `inside`.
///
/// A name already taken fails the job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewFileJob {
    pub inside: PathBuf,
    pub name: OsString,
}

/// The job that renames `path` to `name`.
pub fn rename(path: PathBuf, name: OsString) -> Operation {
    Operation::feature(RenameJob { path, name })
}

/// The job that makes the folder `name` inside `inside`.
pub fn new_folder(inside: PathBuf, name: OsString) -> Operation {
    Operation::feature(NewFolderJob { inside, name })
}

/// The job that makes the empty file `name` inside `inside`.
pub fn new_file(inside: PathBuf, name: OsString) -> Operation {
    Operation::feature(NewFileJob { inside, name })
}

/// Whether `name` names one entry inside a folder: not empty, not `.` or
/// `..`, and without a `/` or a NUL byte.
pub fn is_plain_name(name: &OsStr) -> bool {
    let bytes = name.as_bytes();
    !matches!(bytes, b"" | b"." | b"..") && !bytes.contains(&SEPARATOR) && !bytes.contains(&NUL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_names_of_one_entry() {
        assert!(is_plain_name(OsStr::new("notes.txt")));
        assert!(is_plain_name(OsStr::new(".hidden")));
        assert!(is_plain_name(OsStr::new("...")));
        assert!(is_plain_name(OsStr::from_bytes(b"caf\xe9")));
    }

    #[test]
    fn refuses_names_that_leave_the_folder_or_name_none() {
        for name in ["", ".", "..", "a/b", "/", "../x"] {
            assert!(!is_plain_name(OsStr::new(name)), "{name:?}");
        }
        assert!(!is_plain_name(OsStr::from_bytes(b"a\0b")));
    }
}
