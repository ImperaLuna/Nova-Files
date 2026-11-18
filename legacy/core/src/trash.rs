// Follows the freedesktop trash spec.
// https://specifications.freedesktop.org/trash-spec/latest/
mod folder;
mod info;
mod read;
mod sizes;

pub use folder::{TrashFolder, folder_for};
pub use info::{INFO_SUFFIX, info_text, stored_name};
pub use read::{find, info_of, list, trash_of};
pub use sizes::{add_folder_sizes, folder_size};

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use crate::places::Home;

pub const HOME_TRASH: &str = "Trash";
pub const FILES: &str = "files";
pub const INFO: &str = "info";

#[derive(Debug, Clone, Default)]
pub struct Trashes {
    pub home: Option<PathBuf>,
    pub drives: bool,
}

impl Trashes {
    pub fn of(home: Option<&Home>) -> Self {
        Self {
            home: home.map(|home| home.data.join(HOME_TRASH)),
            drives: home.is_some_and(|home| home.drive_trashes),
        }
    }

    pub fn all(&self) -> Vec<TrashFolder> {
        read::trashes(self.home.as_deref(), self.drives)
    }
}

pub fn item_of_info(info: &Path) -> Option<PathBuf> {
    let name = info.file_name()?.as_bytes();
    let stored = name.strip_suffix(INFO_SUFFIX.as_bytes())?;
    let folder = info.parent()?;
    if stored.is_empty() || folder.file_name()? != INFO {
        return None;
    }
    let root = folder.parent()?;
    let stored = OsStr::from_bytes(stored);
    Some(root.join(FILES).join(stored))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_info_file_names_its_entry_in_the_same_trash() {
        assert_eq!(
            item_of_info(Path::new("/t/Trash/info/a.txt.trashinfo")),
            Some(PathBuf::from("/t/Trash/files/a.txt"))
        );
    }

    #[test]
    fn only_info_files_in_an_info_folder_name_an_entry() {
        for path in [
            "/t/Trash/info/a.txt",
            "/t/Trash/files/a.trashinfo",
            "/t/Trash/info/.trashinfo",
            "a.trashinfo",
        ] {
            assert_eq!(item_of_info(Path::new(path)), None, "{path}");
        }
    }
}
