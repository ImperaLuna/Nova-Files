// Follows the freedesktop trash spec.
// https://specifications.freedesktop.org/trash-spec/latest/
mod folder;
mod info;
mod read;
mod sizes;

pub use folder::{TrashFolder, folder_for};
pub use info::{info_text, stored_name};
pub use nova_files_jobs::trash_info::{FILES, INFO, INFO_SUFFIX};
pub use read::{find, info_of, list, trash_of};
pub use sizes::{add_folder_sizes, folder_size};

use std::path::PathBuf;

use crate::places::Home;

pub const HOME_TRASH: &str = "Trash";

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
