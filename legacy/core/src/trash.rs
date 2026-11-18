pub use nova_files_trash_core::{Trashes, find, trash_of};

use crate::places::Home;

pub fn trashes(home: Option<&Home>) -> Trashes {
    Trashes::of(
        home.map(|home| home.data.as_path()),
        home.is_some_and(|home| home.drive_trashes),
    )
}
