//! Trash as a place: the home trash's folder of trashed entries, listed
//! like any folder, with a "Was in" column saying where each came from.
//! There the selection can only be restored (the island's Restore) or,
//! with Delete, deleted for good; Empty Trash deletes everything in every
//! trash. Both ask first on the island, since they can't be undone.
//!
//! Where each entry was comes from the core's list of the trash, asked
//! for when the trash is opened or a job may have changed it; the same
//! answer gives the number of items in all the user's trashes. Restore
//! and Delete name each entry by the path that list gives, which the
//! core matches (it resolves links in the way to the trash). Inside a
//! trashed folder nothing can be changed.
//!
//! Known gaps: entries in other drives' trashes count and are emptied
//! but aren't listed; a trash made while the window shows it empty shows
//! once the window comes back to it.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{Context, SharedString};
use nova_files_api::{Command, Listing, ListingId, Operation, Trashed};

use super::MainWindow;
use crate::island::{self, Confirm, ITEM, ITEMS};

/// Where the home trash is under the user's data folder, and its folder of
/// trashed entries.
const TRASH_FILES: &str = "Trash/files";
/// The user's data folder under the home folder, when the environment
/// names none.
const DATA_IN_HOME: &str = ".local/share";
const DELETE_LABEL: &str = "Delete";

/// What the window knows of the trash.
#[derive(Debug, Default)]
pub struct TrashState {
    /// The home trash's folder of trashed entries.
    pub folder: PathBuf,
    /// The home trash's entries as the core listed them, by their name
    /// in `folder`.
    listed: HashMap<OsString, Trashed>,
    /// How many items all the user's trashes hold, once the core said.
    count: Option<usize>,
}

impl TrashState {
    /// The trash of the user whose home folder is `home`, and whose data
    /// folder is `data` when the environment names one.
    pub fn new(home: &Path, data: Option<PathBuf>) -> Self {
        let data = data.unwrap_or_else(|| home.join(DATA_IN_HOME));
        Self {
            folder: data.join(TRASH_FILES),
            ..Self::default()
        }
    }
}

impl MainWindow {
    /// Goes to the trash. Before anything was ever trashed its folder
    /// isn't there; the window then shows it empty (see `missing_trash`).
    pub fn open_trash(&mut self, cx: &mut Context<Self>) {
        let folder = self.trash.folder.clone();
        self.open(&folder);
        cx.notify();
    }

    /// The listing to show when the trash folder asked for as `id` isn't
    /// there: an empty one, since nothing was ever trashed.
    pub(super) fn missing_trash(&self, id: ListingId, path: &Path) -> Option<Listing> {
        (path == self.trash.folder).then(|| Listing {
            id,
            path: path.to_path_buf(),
            entries: Arc::from([]),
            folders: None,
        })
    }

    /// Whether the trash is the folder on screen.
    pub fn trash_open(&self) -> bool {
        self.folder() == Some(self.trash.folder.as_path())
    }

    /// Whether the folder on screen is the trash or a folder in it.
    pub fn in_trash(&self) -> bool {
        self.folder()
            .is_some_and(|folder| folder.starts_with(&self.trash.folder))
    }

    /// How many items the user's trashes hold, once known.
    pub const fn trash_count(&self) -> Option<usize> {
        self.trash.count
    }

    /// Asks the core what the trash holds.
    pub(super) fn list_trash(&self) {
        if let Some(connection) = &self.connection {
            connection.send(Command::ListTrash).ok();
        }
    }

    /// Takes what the trash holds.
    pub(super) fn trash_listed(&mut self, items: Vec<Trashed>, cx: &mut Context<Self>) {
        self.trash.count = Some(items.len());
        // The core names entries by their real path, which may differ
        // from `folder` by a link on the way.
        self.trash.listed = items
            .into_iter()
            .filter(|item| {
                item.item
                    .parent()
                    .is_some_and(|parent| parent.ends_with(TRASH_FILES))
            })
            .filter_map(|item| Some((item.item.file_name()?.to_os_string(), item)))
            .collect();
        cx.notify();
    }

    /// Where row `index` of the trash was trashed from, as a user reads it.
    pub fn was_in(&self, index: usize) -> Option<SharedString> {
        let entry = self.listing.as_ref()?.entries.get(index)?;
        let folder = self.trash.listed.get(&entry.name)?.original.parent()?;
        Some(island::short_path(folder, &self.home).into())
    }

    /// The entries selected in the trash, as the core names them.
    fn selected_trashed(&self) -> Vec<PathBuf> {
        if !self.trash_open() {
            return Vec::new();
        }
        self.selected_paths()
            .into_iter()
            .map(|path| {
                path.file_name()
                    .and_then(|name| self.trash.listed.get(name))
                    .map_or(path, |item| item.item.clone())
            })
            .collect()
    }

    /// After a job doing `operation`, which may have changed the trash:
    /// asks again what it holds, and reads it again when it is on screen
    /// (the core names the trashed entries, not the trash, as changed).
    pub(super) fn trash_maybe_changed(&mut self, operation: &Operation) {
        let changes_trash = matches!(
            operation,
            Operation::Trash { .. }
                | Operation::Restore { .. }
                | Operation::DeleteFromTrash { .. }
                | Operation::EmptyTrash
                | Operation::Undo { .. }
        );
        if !changes_trash {
            return;
        }
        self.list_trash();
        if self.trash_open() && !self.waiting() {
            let folder = self.trash.folder.clone();
            self.request(&folder);
        }
    }

    /// Puts the entries selected in the trash back where they were.
    pub(crate) fn restore_selected(&mut self) {
        let items = self.selected_trashed();
        if !items.is_empty() {
            self.start_job(Operation::Restore { items });
        }
    }

    /// Asks on the island before deleting the entries selected in the
    /// trash for good.
    pub(crate) fn delete_selected_for_good(&mut self, cx: &mut Context<Self>) {
        let items = self.selected_trashed();
        if items.is_empty() {
            return;
        }
        let what = island::count(items.len(), ITEM, ITEMS);
        self.ask(
            format!("Delete {what} for good?"),
            Operation::DeleteFromTrash { items },
            cx,
        );
    }

    /// Asks on the island before deleting everything in the trash.
    pub fn ask_empty_trash(&mut self, cx: &mut Context<Self>) {
        let what = self.trash.count.map_or_else(
            || "everything".to_owned(),
            |count| island::count(count, ITEM, ITEMS),
        );
        self.ask(
            format!("Delete {what} in Trash for good?"),
            Operation::EmptyTrash,
            cx,
        );
    }

    fn ask(&mut self, label: String, operation: Operation, cx: &mut Context<Self>) {
        self.island.confirm = Some(Confirm {
            label: label.into(),
            yes: DELETE_LABEL,
            operation,
        });
        cx.notify();
    }

    /// Answers the island's question: does what it asked about, or not.
    pub(crate) fn answer_confirm(&mut self, yes: bool, cx: &mut Context<Self>) {
        if let Some(confirm) = self.island.confirm.take()
            && yes
        {
            self.start_job(confirm.operation);
        }
        cx.notify();
    }

    /// The question on the island, if any.
    pub const fn confirm(&self) -> Option<&Confirm> {
        self.island.confirm.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_trash_is_under_the_data_folder() {
        let home = Path::new("/home/a");
        assert_eq!(
            TrashState::new(home, None).folder,
            Path::new("/home/a/.local/share/Trash/files")
        );
        assert_eq!(
            TrashState::new(home, Some(PathBuf::from("/data"))).folder,
            Path::new("/data/Trash/files")
        );
    }
}
