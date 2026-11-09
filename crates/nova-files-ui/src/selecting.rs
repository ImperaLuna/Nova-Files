//! The window's selection: what clicks and keys select, and what Enter
//! opens. The rules live in `selection`; this is where the window applies
//! them.
//!
//! Every change goes through [`MainWindow::reselect`], which works out
//! the new selection from the old one, lets a picker update its name box
//! (see `pick::select`), and scrolls the cursor's row into view. In a
//! picker a range holds only what it picks; a main window picks anything.
//!
//! When a listing arrives, a fresh read of the same folder keeps the
//! selection by name; arriving in the folder that holds the one just left
//! (Up, Back, or any other way) selects the folder left, so the keyboard
//! carries on from there. Anything else starts with nothing selected.

use gpui::{ClickEvent, Context, ScrollStrategy};
use nova_files_api::{Command, EntryId, EntryKind, Listing, ListingId};

use crate::selection::{Click, Move, Selection, entry_at};
use crate::window::MainWindow;

/// The clicks in a double-click.
pub const DOUBLE_CLICK: usize = 2;

impl MainWindow {
    /// The entries selected, in the listing they belong to, while there
    /// are any.
    pub fn selection(&self) -> Option<(ListingId, Vec<EntryId>)> {
        let selection = self.selection.as_ref()?;
        let ids = selection.ids();
        (!ids.is_empty()).then(|| (selection.listing, ids.to_vec()))
    }

    /// Whether the row `entry` shows as selected.
    pub fn selects(&self, (listing, id): (ListingId, EntryId)) -> bool {
        self.selection
            .as_ref()
            .is_some_and(|selection| selection.listing == listing && selection.contains(id))
    }

    /// The row the keyboard is on, if any.
    pub fn cursor(&self) -> Option<usize> {
        let selection = self.selection.as_ref()?;
        (self.listing_id() == Some(selection.listing))
            .then(|| usize::try_from(selection.cursor().0).ok())
            .flatten()
    }

    /// Replaces the selection with what `change` makes of it, given which
    /// entries a range may hold.
    pub(crate) fn reselect(
        &mut self,
        change: impl FnOnce(Option<Selection>, &dyn Fn(EntryId) -> bool) -> Option<Selection>,
        cx: &mut Context<Self>,
    ) {
        let before = self.selection.take();
        let wanted = self.pick.as_ref().map(crate::pick::Picker::wanted);
        let entries = self
            .listing
            .as_ref()
            .map(|listing| &*listing.entries)
            .unwrap_or_default();
        let picks = |id: EntryId| {
            wanted.is_none_or(|wanted| {
                entry_at(entries, id).is_some_and(|entry| entry.kind == wanted)
            })
        };
        self.selection = change(before, &picks);
        self.pick_selection_changed(cx);
        if let Some(cursor) = self.cursor() {
            self.scroll.scroll_to_item(cursor, ScrollStrategy::Nearest);
        }
        cx.notify();
    }

    /// A click on the entry `id` of `listing` in a main window: selects as
    /// the keys held say, and a double-click opens it.
    pub(crate) fn main_clicked(
        &mut self,
        (listing, id): (ListingId, EntryId),
        event: &ClickEvent,
        cx: &mut Context<Self>,
    ) {
        if event.click_count() >= DOUBLE_CLICK {
            self.open_entries(&[id]);
            return;
        }
        let click = Click::of(event.modifiers(), true);
        self.reselect(
            |before, picks| Some(Selection::clicked(before, listing, id, click, picks)),
            cx,
        );
    }

    /// Moves the cursor as `step` says and selects the row it lands on,
    /// or with `extend` the range from the anchor to it.
    pub(crate) fn move_cursor(&mut self, step: Move, extend: bool, cx: &mut Context<Self>) {
        let Some(listing) = self.listing_id() else {
            return;
        };
        let len = self.entry_count();
        let page = self.rows_on_screen();
        let Some(to) = step
            .to(self.cursor(), len, page)
            .and_then(|to| u32::try_from(to).ok())
        else {
            return;
        };
        self.reselect(
            |before, picks| {
                Some(Selection::moved(
                    before,
                    listing,
                    EntryId(to),
                    extend,
                    picks,
                ))
            },
            cx,
        );
    }

    /// Selects every entry the view picks.
    pub(crate) fn select_all(&mut self, cx: &mut Context<Self>) {
        let Some(listing) = self.listing_id() else {
            return;
        };
        let Ok(len) = u32::try_from(self.entry_count()) else {
            return;
        };
        self.reselect(
            |before, picks| Selection::all(before.as_ref(), listing, len, picks),
            cx,
        );
    }

    /// Adds the entry at the cursor or takes it out.
    pub(crate) fn toggle_cursor(&mut self, cx: &mut Context<Self>) {
        self.reselect(
            |before, _| {
                before.map(|mut selection| {
                    selection.toggle_cursor();
                    selection
                })
            },
            cx,
        );
    }

    /// Selects nothing.
    pub(crate) fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.reselect(|_, _| None, cx);
    }

    /// Opens what is selected: a lone folder is gone into, and files open
    /// in their default apps.
    pub(crate) fn open_selected(&mut self) {
        let ids = self
            .selection()
            .filter(|(listing, _)| self.listing_id() == Some(*listing))
            .map(|(_, ids)| ids)
            .unwrap_or_default();
        self.open_entries(&ids);
    }

    /// Opens the entries `ids` of the listing on screen: a lone folder is
    /// gone into, and files open in their default apps; folders among
    /// several are left alone.
    fn open_entries(&mut self, ids: &[EntryId]) {
        let Some(listing) = &self.listing else { return };
        let entries: Vec<_> = ids
            .iter()
            .filter_map(|&id| entry_at(&listing.entries, id))
            .collect();
        if let [folder] = entries[..]
            && folder.kind == EntryKind::Folder
        {
            let id = folder.id;
            self.open_entry(id);
            return;
        }
        let Some(connection) = &self.connection else {
            return;
        };
        for file in entries.iter().filter(|entry| entry.kind == EntryKind::File) {
            let path = listing.path.join(&file.name);
            // The window can't fall back to the portal; an app that
            // doesn't start shows nothing for now.
            connection
                .send(Command::OpenFile {
                    path,
                    activation: None,
                })
                .ok();
        }
    }

    /// The selection the listing `next` starts with, given the one on
    /// screen before it: see the module's notes.
    pub(crate) fn selection_for(&self, next: &Listing) -> Option<Selection> {
        let shown = self.listing.as_ref()?;
        if shown.path == next.path {
            return self
                .selection
                .as_ref()
                .filter(|selection| selection.listing == shown.id)?
                .carried(&shown.entries, next.id, &next.entries);
        }
        let left = shown.path.file_name()?;
        (shown.path.parent() == Some(next.path.as_path()))
            .then(|| Selection::named(next.id, &next.entries, left))
            .flatten()
    }

    fn entry_count(&self) -> usize {
        self.listing
            .as_ref()
            .map_or(0, |listing| listing.entries.len())
    }

    /// The rows on screen, which a page move steps by.
    pub fn rows_on_screen(&self) -> usize {
        self.rows_drawn.len()
    }

    /// The names of the entries selected in the folder on screen, in
    /// order.
    pub fn selected_names(&self) -> Vec<String> {
        let Some(listing) = &self.listing else {
            return Vec::new();
        };
        self.selection()
            .filter(|(id, _)| *id == listing.id)
            .map(|(_, ids)| ids)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|id| entry_at(&listing.entries, id))
            .map(|entry| entry.name.to_string_lossy().into_owned())
            .collect()
    }
}
