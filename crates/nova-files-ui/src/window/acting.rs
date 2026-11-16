//! What the window does to the files selected, and the island's notes:
//! move them to the trash, delete them for good (after asking), undo,
//! and say which files didn't open. Each becomes one of the core's jobs
//! (see `jobs`); the island says what happened (see `island`). Copy, cut
//! and paste are in `clipboard`.

use std::path::PathBuf;

use gpui::{Context, SharedString};
use nova_files_api::{EntryId, EntryKind, Listing, NotOpened, Operation};

use super::MainWindow;
use crate::island::{self, ITEM, ITEMS, NOTE_TIME, Note, NoteUndo, opening};
use crate::selection::entry_at;

impl MainWindow {
    /// The paths of the entries selected in the list on screen, in list
    /// order, each where it is: a search's results in their own folders.
    pub fn selected_paths(&self) -> Vec<PathBuf> {
        let Some((listing, ids)) = self.selected_ids() else {
            return Vec::new();
        };
        ids.into_iter()
            .filter_map(|id| entry_at(&listing.entries, id))
            .map(|entry| listing.path_of(entry))
            .collect()
    }

    /// The listing on screen and the entries selected in it.
    fn selected_ids(&self) -> Option<(&Listing, Vec<EntryId>)> {
        let listing = self.listing.as_ref()?;
        let (id, ids) = self.selection()?;
        (id == listing.id).then_some((listing, ids))
    }

    /// How many entries are selected in the folder on screen.
    pub fn selected_count(&self) -> usize {
        self.selected_ids().map_or(0, |(_, ids)| ids.len())
    }

    /// What the files selected take, once the core has read the size of
    /// each; folders don't count. `None` while one isn't known yet.
    pub fn selected_bytes(&self) -> Option<u64> {
        let (listing, ids) = self.selected_ids()?;
        ids.into_iter().try_fold(0_u64, |total, id| {
            let entry = entry_at(&listing.entries, id)?;
            if entry.kind == EntryKind::Folder {
                return Some(total);
            }
            let row = usize::try_from(id.0).ok()?;
            let size = self.details.get(&row)?.size?;
            Some(total.saturating_add(size))
        })
    }

    /// Moves the files selected to the trash; in the trash, asks before
    /// deleting them for good.
    pub fn trash_selected(&mut self, cx: &mut Context<Self>) {
        if self.in_trash() {
            self.delete_selected(cx);
            return;
        }
        let paths = self.selected_paths();
        if !paths.is_empty() && self.pick.is_none() {
            self.start_job(Operation::Trash { paths });
        }
    }

    /// Asks on the island before deleting the files selected for good;
    /// in the trash, the entries selected there.
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

    /// Deleting the selection for good, as the job that would do it;
    /// `None` with nothing selected or in a picker.
    pub(super) fn deleting_selected(&self) -> Option<Operation> {
        if self.in_trash() {
            let items = self.selected_trashed();
            return (!items.is_empty()).then_some(Operation::DeleteFromTrash { items });
        }
        deleting(self.selected_paths(), self.pick.is_some())
    }

    /// Undoes what the note names, or else this window's newest job that
    /// can still be undone.
    pub(crate) fn undo(&mut self, cx: &mut Context<Self>) {
        // In a rename's field, Ctrl+Z is the field's.
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

    /// Shows `label` on the island, with an Undo button when `undo` says
    /// what it does. It goes by itself after a while.
    pub fn show_note(
        &mut self,
        label: impl Into<SharedString>,
        undo: Option<NoteUndo>,
        cx: &mut Context<Self>,
    ) {
        self.island.not_opened.clear();
        self.put_note(label.into(), undo, cx);
    }

    /// Says on the island that `path` started no app, and why. A note
    /// already up for other files that didn't open takes it in too.
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

    /// Puts up a note, whose timer replaces, and so cancels, the old
    /// one's.
    fn put_note(&mut self, label: SharedString, undo: Option<NoteUndo>, cx: &mut Context<Self>) {
        self.island.note = Some(Note { label, undo });
        self.island.note_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(NOTE_TIME).await;
            this.update(cx, Self::dismiss_note).ok();
        }));
        cx.notify();
    }

    /// Takes the note down.
    pub(crate) fn dismiss_note(&mut self, cx: &mut Context<Self>) {
        self.island.note = None;
        self.island.note_timer = None;
        self.island.not_opened.clear();
        cx.notify();
    }

    /// The note on the island, if any.
    pub const fn note(&self) -> Option<&Note> {
        self.island.note.as_ref()
    }
}

/// Deleting `paths` for good, as the job that would do it; `None` with
/// none of them, or in a picker, which never deletes.
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
