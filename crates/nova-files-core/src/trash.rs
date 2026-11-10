//! The desktop's trash, as the freedesktop trash specification (1.0)
//! lays it out, shared with every other app.
//!
//! A trash is a folder holding `files/`, where trashed entries sit under
//! a name of their own, and `info/`, where `<that name>.trashinfo` says
//! where each came from and when it was trashed (see `info`). The home
//! trash, `$XDG_DATA_HOME/Trash`, takes what is on its own drive; every
//! other drive has its own, at its top folder (see `folder`), so trashing
//! is always a rename and never a copy. A folder's size goes in the
//! trash's `directorysizes` cache, which other file managers read (see
//! `sizes`).

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

/// The home trash's folder, under the user's data folder.
pub const HOME_TRASH: &str = "Trash";
/// The folder of a trash that holds the trashed entries.
pub const FILES: &str = "files";
/// The folder of a trash that holds the info files.
pub const INFO: &str = "info";

/// Where the user's trashes are.
#[derive(Debug, Clone, Default)]
pub struct Trashes {
    /// The home trash, made when first needed; `None` without a data
    /// folder.
    pub home: Option<PathBuf>,
    /// Whether the trashes at the top of other drives count.
    pub drives: bool,
}

impl Trashes {
    /// The trashes of the user `home` names.
    pub fn of(home: Option<&Home>) -> Self {
        Self {
            home: home.map(|home| home.data.join(HOME_TRASH)),
            drives: home.is_some_and(|home| home.drive_trashes),
        }
    }

    /// Each trash there is now: the home trash, when it is there, and,
    /// when they count, each mounted drive's.
    pub fn all(&self) -> Vec<TrashFolder> {
        read::trashes(self.home.as_deref(), self.drives)
    }
}

/// Where the entry an info file at `info` describes sits in its trash,
/// or `None` when `info` isn't an info file in a trash's `info` folder.
pub fn item_of_info(info: &Path) -> Option<PathBuf> {
    let name = info.file_name()?.as_bytes();
    let stored = name.strip_suffix(INFO_SUFFIX.as_bytes())?;
    let folder = info.parent()?;
    if stored.is_empty() || folder.file_name()? != INFO {
        return None;
    }
    let root = folder.parent()?;
    // From bytes `info`'s own name held, cut at an ASCII suffix.
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
