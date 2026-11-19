use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::plan::Entry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    entry: Entry,
    born: Option<SystemTime>,
}

pub fn of(path: &Path) -> Option<Identity> {
    let there = path.symlink_metadata().ok()?;
    Some(Identity {
        entry: Entry::of(&there),
        born: there.created().ok(),
    })
}

pub fn same(path: &Path, was: Identity) -> bool {
    of(path) == Some(was)
}

pub fn still(paths: Vec<(PathBuf, Identity)>) -> Vec<PathBuf> {
    paths
        .into_iter()
        .filter(|(path, was)| same(path, *was))
        .map(|(path, _)| path)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::test_folder::TestFolder;

    #[test]
    fn a_file_left_alone_is_still_the_same_one() {
        let folder = TestFolder::new("identity-same");
        let file = folder.file("a", "one");
        let was = of(&file).unwrap();

        assert_eq!(still(vec![(file.clone(), was)]), vec![file]);
    }

    #[test]
    fn a_file_put_in_another_place_is_not() {
        let folder = TestFolder::new("identity-replaced");
        let file = folder.file("a", "one");
        let was = of(&file).unwrap();
        fs::remove_file(&file).unwrap();
        folder.file("b", "a longer one");
        fs::rename(folder.path().join("b"), &file).unwrap();

        assert!(!same(&file, was));
        assert_eq!(still(vec![(file, was)]), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_missing_file_has_no_identity() {
        let folder = TestFolder::new("identity-missing");

        assert_eq!(of(&folder.path().join("gone")), None);
    }
}
