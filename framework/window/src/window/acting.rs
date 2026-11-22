use std::path::PathBuf;

use gpui::{Context, SharedString};
use nova_files_api::{EntryId, EntryKind, Listing};
use nova_files_shell::Undo;

use super::MainWindow;
use crate::island::{NOTE_TIME, Note};
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

    pub fn show_note(
        &mut self,
        label: impl Into<SharedString>,
        undo: Option<Undo>,
        cx: &mut Context<Self>,
    ) {
        self.put_note(label.into(), undo, cx);
    }

    fn put_note(&mut self, label: SharedString, undo: Option<Undo>, cx: &mut Context<Self>) {
        self.tell_note_gone(cx);
        self.island.note = Some(Note { label, undo });
        self.island.note_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(NOTE_TIME).await;
            this.update(cx, Self::dismiss_note).ok();
        }));
        cx.notify();
    }

    pub(crate) fn dismiss_note(&mut self, cx: &mut Context<Self>) {
        self.tell_note_gone(cx);
        self.island.note = None;
        self.island.note_timer = None;
        cx.notify();
    }

    fn tell_note_gone(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.island.note_owner.take() {
            self.host
                .island
                .update(cx, |island, cx| island.gone(id, cx));
        }
    }

    pub const fn note(&self) -> Option<&Note> {
        self.island.note.as_ref()
    }
}
