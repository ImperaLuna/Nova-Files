//! What the window does to the files selected, and the island's notes:
//! move them to the trash, and undo. Each becomes one of the core's jobs
//! (see `jobs`); the island says what happened (see `island`).

use std::path::PathBuf;

use gpui::{Context, SharedString};
use nova_files_api::{EntryKind, Operation};

use super::MainWindow;
use crate::island::{NOTE_TIME, Note, NoteUndo};
use crate::selection::entry_at;

impl MainWindow {
    /// The paths of the entries selected in the folder on screen, in list
    /// order.
    pub fn selected_paths(&self) -> Vec<PathBuf> {
        let Some(listing) = &self.listing else {
            return Vec::new();
        };
        self.selection()
            .filter(|(id, _)| *id == listing.id)
            .map(|(_, ids)| ids)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|id| entry_at(&listing.entries, id))
            .map(|entry| listing.path.join(&entry.name))
            .collect()
    }

    /// How many entries are selected in the folder on screen.
    pub fn selected_count(&self) -> usize {
        match (&self.listing, self.selection()) {
            (Some(listing), Some((id, ids))) if id == listing.id => ids.len(),
            _ => 0,
        }
    }

    /// What the files selected take, once the core has read the size of
    /// each; folders don't count. `None` while one isn't known yet.
    pub fn selected_bytes(&self) -> Option<u64> {
        let listing = self.listing.as_ref()?;
        let (_, ids) = self.selection().filter(|(id, _)| *id == listing.id)?;
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

    /// Moves the files selected to the trash.
    pub(crate) fn trash_selected(&mut self) {
        let paths = self.selected_paths();
        if !paths.is_empty() && self.pick.is_none() {
            self.start_job(Operation::Trash { paths });
        }
    }

    /// Undoes what the note names, or else this window's newest job that
    /// can still be undone.
    pub(crate) fn undo(&mut self, cx: &mut Context<Self>) {
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
        self.island.notes_shown += 1;
        let serial = self.island.notes_shown;
        self.island.note = Some(Note {
            label: label.into(),
            undo,
            serial,
        });
        self.island.note_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(NOTE_TIME).await;
            this.update(cx, |this, cx| {
                if this
                    .island
                    .note
                    .as_ref()
                    .is_some_and(|note| note.serial == serial)
                {
                    this.dismiss_note(cx);
                }
            })
            .ok();
        }));
        cx.notify();
    }

    /// Takes the note down.
    pub(crate) fn dismiss_note(&mut self, cx: &mut Context<Self>) {
        self.island.note = None;
        self.island.note_timer = None;
        cx.notify();
    }

    /// The note on the island, if any.
    pub const fn note(&self) -> Option<&Note> {
        self.island.note.as_ref()
    }
}
