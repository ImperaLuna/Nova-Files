use std::io;
use std::path::{Path, PathBuf};

use nova_files_api::numbered::NUMBERS;
pub use nova_files_api::numbered::{Naming, numbered};

use crate::place::rename_no_replace;

pub fn free_path(taken: &Path, naming: Naming) -> io::Result<PathBuf> {
    let (Some(folder), Some(name)) = (taken.parent(), taken.file_name()) else {
        return Err(io::ErrorKind::InvalidInput.into());
    };
    for number in NUMBERS {
        let candidate = folder.join(numbered(name, number, naming));
        match candidate.symlink_metadata() {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(candidate),
            Err(error) => return Err(error),
            Ok(_) => {}
        }
    }
    Err(io::ErrorKind::AlreadyExists.into())
}

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
    use crate::test_folder::TestFolder;

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
