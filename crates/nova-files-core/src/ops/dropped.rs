//! What dragging files onto a folder does, as other file managers do it:
//! files on the folder's own drive move, files from another drive are
//! copied, so a drag never takes files off the drive they were on.
//!
//! A drive here is a filesystem: two paths are on one drive when the
//! system gives them the same device. Each source is looked at itself,
//! never through a link (a drive mounted on a folder dragged counts as
//! that drive), and the folder through any link to it. A source or a
//! folder that can't be looked at makes the drop a copy, which leaves
//! every source where it was. This looks at the disk: the caller runs it
//! off the core's loop (see `Jobs::start_drop`).

use std::fs;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use nova_files_api::Operation;

/// The job dropping `sources` onto the folder `into` runs.
pub fn dropped(sources: Vec<PathBuf>, into: PathBuf) -> Operation {
    if on_one_drive(&sources, &into) {
        Operation::Move { sources, into }
    } else {
        Operation::Copy { sources, into }
    }
}

/// Whether every one of `sources` is on the drive of the folder `into`.
fn on_one_drive(sources: &[PathBuf], into: &Path) -> bool {
    let Ok(drive) = fs::metadata(into).map(|folder| folder.dev()) else {
        return false;
    };
    sources.iter().all(|source| {
        source
            .symlink_metadata()
            .is_ok_and(|source| source.dev() == drive)
    })
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;
    use crate::ops::test_folder::TestFolder;

    /// `/proc` is never on the drive of a temp folder.
    const ANOTHER_DRIVE: &str = "/proc";

    #[test]
    fn files_on_the_folders_drive_move() {
        let root = TestFolder::new("drop-move");
        let file = root.file("a", "a");
        let into = root.path().join("in");
        fs::create_dir(&into).unwrap();
        assert_eq!(
            dropped(vec![file.clone()], into.clone()),
            Operation::Move {
                sources: vec![file],
                into
            }
        );
    }

    #[test]
    fn files_from_another_drive_are_copied() {
        let root = TestFolder::new("drop-copy");
        let source = PathBuf::from(ANOTHER_DRIVE).join("self");
        let into = root.path().to_path_buf();
        assert_eq!(
            dropped(vec![source.clone()], into.clone()),
            Operation::Copy {
                sources: vec![source],
                into
            }
        );
    }

    #[test]
    fn a_source_or_folder_that_isnt_there_copies() {
        let root = TestFolder::new("drop-gone");
        let file = root.file("a", "a");
        let gone = root.path().join("gone");
        assert!(!on_one_drive(&[file.clone(), gone.clone()], root.path()));
        assert!(!on_one_drive(&[file], &gone));
    }

    #[test]
    fn a_link_counts_where_it_is_and_the_folder_where_it_points() {
        let root = TestFolder::new("drop-links");
        let link = root.path().join("link");
        symlink(ANOTHER_DRIVE, &link).unwrap();
        assert!(on_one_drive(std::slice::from_ref(&link), root.path()));
        let file = root.file("a", "a");
        assert!(!on_one_drive(&[file], &link));
    }
}
