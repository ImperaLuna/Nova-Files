use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{App, Context, SharedString};
use nova_files_api::{Command, Listing, ListingId, Operation, Trashed};
use nova_files_shell::{FolderRules, Views};

use super::MainWindow;
use crate::island::{self, Confirm, ITEM, ITEMS};

const TRASH_FILES: &str = "Trash/files";
const DATA_IN_HOME: &str = ".local/share";
const DELETE_LABEL: &str = "Delete";
const TRASH_RULES_OWNER: &str = "trash";

#[derive(Debug, Default)]
pub struct TrashState {
    pub folder: PathBuf,
    listed: HashMap<OsString, Trashed>,
    count: Option<usize>,
}

impl TrashState {
    pub fn new(home: &Path, data: Option<PathBuf>) -> Self {
        let data = data.unwrap_or_else(|| home.join(DATA_IN_HOME));
        Self {
            folder: data.join(TRASH_FILES),
            ..Self::default()
        }
    }
}

impl MainWindow {
    pub fn open_trash(&mut self, cx: &mut Context<Self>) {
        let folder = self.trash.folder.clone();
        self.open(&folder);
        cx.notify();
    }

    pub(super) fn missing_trash(&self, id: ListingId, path: &Path) -> Option<Listing> {
        (path == self.trash.folder).then(|| Listing {
            id,
            path: path.to_path_buf(),
            entries: Arc::from([]),
            folders: None,
        })
    }

    pub fn trash_open(&self) -> bool {
        self.folder() == Some(self.trash.folder.as_path())
    }

    pub(super) fn claim_trash_rules(&self) {
        self.host.rules.claim(
            TRASH_RULES_OWNER,
            self.trash.folder.clone(),
            FolderRules {
                read_only: true,
                views: Views::DetailsOnly,
                replaces_menu: true,
            },
        );
    }

    pub fn in_trash(&self) -> bool {
        self.folder()
            .is_some_and(|folder| folder.starts_with(&self.trash.folder))
    }

    pub const fn trash_count(&self) -> Option<usize> {
        self.trash.count
    }

    pub(super) fn list_trash(&self) {
        if let Some(connection) = &self.connection {
            connection.send(Command::ListTrash).ok();
        }
    }

    pub(super) fn trash_listed(&mut self, items: Vec<Trashed>, cx: &mut Context<Self>) {
        self.trash.count = Some(items.len());
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

    pub fn was_in(&self, index: usize) -> Option<SharedString> {
        let entry = self.listing.as_ref()?.entries.get(index)?;
        let folder = self.trash.listed.get(&entry.name)?.original.parent()?;
        Some(island::short_path(folder, &self.home).into())
    }

    pub(super) fn selected_trashed(&self) -> Vec<PathBuf> {
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

    pub(super) fn trash_maybe_changed(&mut self, operation: &Operation, replaced: bool) {
        let changes_trash = replaced
            || matches!(
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

    pub(crate) fn restore_selected(&mut self) {
        let items = self.selected_trashed();
        if !items.is_empty() {
            self.start_job(Operation::Restore { items });
        }
    }

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

    pub(super) fn ask(&mut self, label: String, operation: Operation, cx: &mut Context<Self>) {
        let window = cx.entity().downgrade();
        let on_yes = move |cx: &mut App| {
            window
                .update(cx, |this, cx| {
                    let deletes = matches!(
                        operation,
                        Operation::Delete { .. } | Operation::DeleteFromTrash { .. }
                    );
                    if !deletes || this.deleting_selected().as_ref() == Some(&operation) {
                        this.start_job(operation);
                    }
                    cx.notify();
                })
                .ok();
        };
        self.island.confirm = Some(Confirm {
            label: label.into(),
            yes: DELETE_LABEL,
            on_yes: Box::new(on_yes),
        });
        cx.notify();
    }

    pub(crate) fn answer_confirm(&mut self, yes: bool, cx: &mut Context<Self>) {
        if let Some(confirm) = self.island.confirm.take()
            && yes
        {
            cx.defer(confirm.on_yes);
        }
        cx.notify();
    }

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
