//! Names for "keep both": the first free name of the form `name (2).ext`.
//!
//! A file's number goes before its extension, the part from its last dot,
//! or from `.tar.` for compressed tarballs, so `backup.tar.gz` becomes
//! `backup (2).tar.gz`. A name whose only dot leads it (`.bashrc`) has no
//! extension, and neither has a folder. Names are handled as bytes, so one
//! that isn't UTF-8 keeps its bytes. [`take_free_name`] gives an entry
//! the first such name that is free.

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Path, PathBuf};

use super::place::rename_no_replace;

/// Extensions that sit on top of `.tar` and count as one with it.
const TARBALLS: [&[u8]; 4] = [b".tar.gz", b".tar.xz", b".tar.zst", b".tar.bz2"];
/// The first number tried; the original is the first.
const FIRST: u32 = 2;
/// The most numbers tried before giving up: a folder this crowded with
/// copies of one name is not one a person keeps.
const LAST: u32 = 10_000;

/// Whether the name is a folder's, which never has an extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Naming {
    File,
    Folder,
}

/// `name` with `number` added, as "keep both" names its copy.
pub fn numbered(name: &OsStr, number: u32, naming: Naming) -> OsString {
    let bytes = name.as_bytes();
    let split = match naming {
        Naming::Folder => bytes.len(),
        Naming::File => extension_start(bytes).unwrap_or(bytes.len()),
    };
    let (stem, extension) = bytes.split_at(split);
    let mut numbered = stem.to_vec();
    numbered.extend(format!(" ({number})").into_bytes());
    numbered.extend(extension);
    OsString::from_vec(numbered)
}

/// Where the extension of a file named `name` starts, if it has one.
fn extension_start(name: &[u8]) -> Option<usize> {
    let lower = name.to_ascii_lowercase();
    if let Some(tarball) = TARBALLS.iter().find(|tarball| lower.ends_with(tarball)) {
        let start = name.len() - tarball.len();
        if start > 0 {
            return Some(start);
        }
    }
    name.iter()
        .rposition(|&byte| byte == b'.')
        .filter(|&dot| dot > 0)
}

/// The first free path next to `taken` for "keep both".
///
/// # Errors
/// When the folder can't be looked in, or every number is taken.
pub fn free_path(taken: &Path, naming: Naming) -> io::Result<PathBuf> {
    let (Some(folder), Some(name)) = (taken.parent(), taken.file_name()) else {
        return Err(io::ErrorKind::InvalidInput.into());
    };
    for number in FIRST..=LAST {
        let candidate = folder.join(numbered(name, number, naming));
        match candidate.symlink_metadata() {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(candidate),
            Err(error) => return Err(error),
            Ok(_) => {}
        }
    }
    Err(io::ErrorKind::AlreadyExists.into())
}

/// Gives `from` the name `wanted`, or the first free one beside it for
/// "keep both", and returns the path it took.
///
/// # Errors
/// When a rename fails for any reason but a taken name, or every number
/// is taken.
pub fn take_free_name(from: &Path, wanted: &Path) -> io::Result<PathBuf> {
    let mut to = wanted.to_path_buf();
    loop {
        let Err(error) = rename_no_replace(from, &to) else {
            return Ok(to);
        };
        if error.kind() != io::ErrorKind::AlreadyExists {
            return Err(error);
        }
        to = free_path(wanted, Naming::File)?;
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::ops::test_folder::TestFolder;

    fn file(name: &str, number: u32) -> OsString {
        numbered(OsStr::new(name), number, Naming::File)
    }

    #[test]
    fn numbers_go_before_the_extension() {
        assert_eq!(file("photo.jpg", 2), "photo (2).jpg");
        assert_eq!(file("notes", 3), "notes (3)");
        assert_eq!(file("a.b.c", 2), "a.b (2).c");
        assert_eq!(file(".bashrc", 2), ".bashrc (2)");
        assert_eq!(file(".config.toml", 2), ".config (2).toml");
        assert_eq!(file("backup.tar.gz", 2), "backup (2).tar.gz");
        assert_eq!(file("Backup.TAR.ZST", 2), "Backup (2).TAR.ZST");
        assert_eq!(file("x.tar.xz", 2), "x (2).tar.xz");
        assert_eq!(file("y.tar.bz2", 2), "y (2).tar.bz2");
        assert_eq!(file(".tar.gz", 2), ".tar (2).gz");
        assert_eq!(file("plain.tar", 2), "plain (2).tar");
        assert_eq!(
            numbered(OsStr::new("my.photos"), 2, Naming::Folder),
            "my.photos (2)"
        );
        assert_eq!(
            numbered(OsStr::from_bytes(b"caf\xe9.txt"), 2, Naming::File).as_bytes(),
            b"caf\xe9 (2).txt"
        );
    }

    #[test]
    fn finds_the_first_free_number() {
        let folder = TestFolder::new("keep-both");
        let taken = folder.file("a.txt", "");
        folder.file("a (2).txt", "");

        assert_eq!(
            free_path(&taken, Naming::File).unwrap(),
            folder.path().join("a (3).txt")
        );
        assert_eq!(
            free_path(Path::new("/"), Naming::File).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn a_folder_that_cant_be_looked_in_is_an_error() {
        let folder = TestFolder::new("keep-both-not-a-folder");
        let file = folder.file("a", "");

        assert!(free_path(&file.join("b"), Naming::File).is_err());
    }

    #[test]
    fn a_taken_name_gives_the_next_free_one() {
        let folder = TestFolder::new("keep-both-take");
        let part = folder.path().join(".part");
        fs::write(&part, b"zip").unwrap();
        fs::write(folder.path().join("a.zip"), b"mine").unwrap();
        let placed = take_free_name(&part, &folder.path().join("a.zip")).unwrap();
        assert_eq!(placed, folder.path().join("a (2).zip"));
        assert_eq!(fs::read(&placed).unwrap(), b"zip");
        assert_eq!(fs::read(folder.path().join("a.zip")).unwrap(), b"mine");
    }

    #[test]
    fn a_name_that_cant_be_given_is_an_error() {
        let folder = TestFolder::new("keep-both-take-missing");
        let part = folder.file(".part", "zip");
        let error = take_free_name(&part, &folder.path().join("gone/a.zip")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }
}
