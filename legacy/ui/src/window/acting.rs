use std::path::PathBuf;

use gpui::{Context, SharedString};
use nova_files_api::{EntryId, EntryKind, Listing, Operation};
use nova_files_open_api::NotOpened;

use super::MainWindow;
use crate::island::{self, ITEM, ITEMS, NOTE_TIME, Note, NoteUndo, opening};
use crate::selection::entry_at;

impl MainWindow {
    pub fn selected_paths(&self) -> Vec<PathBuf> {
        let Some((listing, ids)) = self.selected_ids() else {
            return Vec::new();
        };
        ids.into_iter()
            .filter_map(|id| entry_at(&listing.entries, id))
            .map(|entry| listing.path_of(entry))
            .collect()
    }

    fn selected_ids(&self) -> Option<(&Listing, Vec<EntryId>)> {
        let listing = self.listing.as_ref()?;
        let (id, ids) = self.selection()?;
        (id == listing.id).then_some((listing, ids))
    }

    pub fn selected_count(&self) -> usize {
        self.selected_ids().map_or(0, |(_, ids)| ids.len())
    }

    pub fn selected_bytes(&self) -> Option<u64> {
        let (listing, ids) = self.selected_ids()?;
        ids.into_iter().try_fold(0_u64, |total, id| {
            let entry = entry_at(&listing.entries, id)?;
            if entry.kind == EntryKind::Folder {
                return Some(total);
            }
            let row = usize::try_from(id.0).ok()?;
            let size = self.details.get(&row)?.details.size?;
            Some(total.saturating_add(size))
        })
    }

    pub fn trash_selected(&mut self, cx: &mut Context<Self>) {
        if self.folder_rules().trashed {
            self.delete_selected(cx);
            return;
        }
        let paths = self.selected_paths();
        if !paths.is_empty() && self.pick.is_none() {
            self.start_job(Operation::Trash { paths });
        }
    }

    pub fn delete_selected(&mut self, cx: &mut Context<Self>) {
        let Some(operation) = self.deleting_selected() else {
            return;
        };
        let many = match &operation {
            Operation::Delete { paths } => paths.len(),
            Operation::DeleteFromTrash { items } => items.len(),
            _ => return,
        };
        let what = island::count(many, ITEM, ITEMS);
        self.ask(format!("Delete {what} for good?"), operation, cx);
    }

    pub(super) fn deleting_selected(&self) -> Option<Operation> {
        if self.folder_rules().trashed {
            let named = self
                .folder()
                .and_then(|folder| self.host.rules.named(folder));
            let items = named.map(|_| self.selected_paths()).unwrap_or_default();
            return (!items.is_empty()).then_some(Operation::DeleteFromTrash { items });
        }
        deleting(self.selected_paths(), self.pick.is_some())
    }

    pub(crate) fn undo(&mut self, cx: &mut Context<Self>) {
        if self.renaming() {
            return;
        }
        let undo = self
            .island
            .note
            .as_ref()
            .and_then(|note| note.undo.clone())
            .or_else(|| self.jobs.last_undoable().map(NoteUndo::Job));
        self.dismiss_note(cx);
        match undo {
            Some(NoteUndo::Job(job)) => {
                self.start_job(Operation::Undo { job });
            }
            Some(NoteUndo::Send(commands)) => {
                if let Some(connection) = &self.connection {
                    for command in commands {
                        connection.send(command).ok();
                    }
                }
            }
            None => {}
        }
    }

    pub fn show_note(
        &mut self,
        label: impl Into<SharedString>,
        undo: Option<NoteUndo>,
        cx: &mut Context<Self>,
    ) {
        self.island.not_opened.clear();
        self.put_note(label.into(), undo, cx);
    }

    pub(crate) fn open_event(&mut self, event: nova_files_open_api::Event, cx: &mut Context<Self>) {
        match event {
            nova_files_open_api::Event::Opened { .. } => {}
            nova_files_open_api::Event::NotOpened { path, reason } => {
                self.file_not_opened(path, reason, cx);
            }
            nova_files_open_api::Event::OpenWith(answer) => self.open_with_answered(answer, cx),
        }
    }

    pub(crate) fn file_not_opened(
        &mut self,
        path: PathBuf,
        reason: NotOpened,
        cx: &mut Context<Self>,
    ) {
        let failed = &mut self.island.not_opened;
        failed.retain(|(was, _)| *was != path);
        failed.push((path, reason));
        if let Some(label) = opening::note(failed) {
            self.put_note(label.into(), None, cx);
        }
    }

    fn put_note(&mut self, label: SharedString, undo: Option<NoteUndo>, cx: &mut Context<Self>) {
        self.island.note = Some(Note { label, undo });
        self.island.note_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(NOTE_TIME).await;
            this.update(cx, Self::dismiss_note).ok();
        }));
        cx.notify();
    }

    pub(crate) fn dismiss_note(&mut self, cx: &mut Context<Self>) {
        self.island.note = None;
        self.island.note_timer = None;
        self.island.not_opened.clear();
        cx.notify();
    }

    pub const fn note(&self) -> Option<&Note> {
        self.island.note.as_ref()
    }
}

fn deleting(paths: Vec<PathBuf>, picking: bool) -> Option<Operation> {
    (!paths.is_empty() && !picking).then_some(Operation::Delete { paths })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deleting_needs_a_selection_and_no_picker() {
        let paths = vec![PathBuf::from("/home/a/notes.txt")];
        assert_eq!(
            deleting(paths.clone(), false),
            Some(Operation::Delete {
                paths: paths.clone()
            })
        );
        assert_eq!(deleting(paths, true), None);
        assert_eq!(deleting(Vec::new(), false), None);
    }
}
