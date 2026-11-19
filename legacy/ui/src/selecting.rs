use gpui::{ClickEvent, Context, KeyDownEvent};
use nova_files_api::{Command, EntryId, EntryKind, Listing, ListingId};

use crate::jump;
use crate::selection::{Click, Move, Selection, entry_at};
use crate::window::MainWindow;

pub const DOUBLE_CLICK: usize = 2;

impl MainWindow {
    pub fn selection(&self) -> Option<(ListingId, Vec<EntryId>)> {
        let selection = self.selection.as_ref()?;
        let ids = selection.ids();
        (!ids.is_empty()).then(|| (selection.listing, ids.to_vec()))
    }

    pub fn selects(&self, (listing, id): (ListingId, EntryId)) -> bool {
        self.selection
            .as_ref()
            .is_some_and(|selection| selection.listing == listing && selection.contains(id))
    }

    pub fn cursor(&self) -> Option<usize> {
        let selection = self.selection.as_ref()?;
        (self.listing_id() == Some(selection.listing))
            .then(|| usize::try_from(selection.cursor().0).ok())
            .flatten()
    }

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
        let parts = &self.parts;
        let picks = |id: EntryId| {
            let entry = entry_at(entries, id);
            wanted.is_none_or(|wanted| entry.is_some_and(|entry| entry.kind == wanted))
                && entry.is_none_or(|entry| parts.selectable(entry, cx))
        };
        let several =
            self.pick.as_ref().is_none_or(crate::pick::Picker::several) && self.parts.several(cx);
        self.selection = change(before, &picks).map(|selection| {
            if several {
                selection
            } else {
                selection.at_most_one()
            }
        });
        self.publish(cx);
        self.pick_selection_changed(cx);
        self.preview_follow_cursor(cx);
        if let Some(cursor) = self.cursor() {
            self.scroll_to_entry(cursor);
        }
        cx.notify();
    }

    pub(crate) fn main_clicked(
        &mut self,
        (listing, id): (ListingId, EntryId),
        event: &ClickEvent,
        cx: &mut Context<Self>,
    ) {
        if event.click_count() >= DOUBLE_CLICK {
            self.open_entries(&[id], cx);
            return;
        }
        let click = Click::of(event.modifiers(), true);
        self.reselect(
            |before, picks| Some(Selection::clicked(before, listing, id, click, picks)),
            cx,
        );
    }

    pub(crate) fn move_cursor(&mut self, step: Move, extend: bool, cx: &mut Context<Self>) {
        self.move_cursor_to(self.cursor(), step, extend, cx);
    }

    pub(crate) fn move_cursor_from(
        &mut self,
        from: Option<usize>,
        step: Move,
        cx: &mut Context<Self>,
    ) {
        self.move_cursor_to(from, step, false, cx);
    }

    fn move_cursor_to(
        &mut self,
        from: Option<usize>,
        step: Move,
        extend: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(listing) = self.listing_id() else {
            return;
        };
        let len = self.entry_count();
        let page = self.rows_on_screen();
        let per_line = self.grid().per_line;
        let Some(to) = step
            .to(from, len, (page, per_line))
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

    pub(crate) fn jumping(&self, cx: &Context<Self>) -> bool {
        self.pick.is_none() && self.jump.continues(cx.background_executor().now())
    }

    pub(crate) fn jump_typed(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let held = event.keystroke.modifiers;
        if self.pick.is_some() || held.control || held.alt || held.platform || held.function {
            return false;
        }
        let Some(typed) = event.keystroke.key_char.as_deref() else {
            return false;
        };
        let now = cx.background_executor().now();
        let blank = typed.chars().all(char::is_whitespace);
        if typed.chars().any(char::is_control) || (blank && !self.jump.continues(now)) {
            return false;
        }
        let Some(listing) = &self.listing else {
            return false;
        };
        let (prefix, past) = self.jump.typed(typed, now);
        let names = listing.entries.iter().map(|entry| entry.name.as_os_str());
        let found = jump::find(names, &prefix, self.cursor(), past);
        let listing = listing.id;
        if let Some(row) = found.and_then(|row| u32::try_from(row).ok()) {
            self.reselect(
                |before, picks| {
                    Some(Selection::moved(
                        before,
                        listing,
                        EntryId(row),
                        false,
                        picks,
                    ))
                },
                cx,
            );
        }
        true
    }

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

    pub(crate) fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.reselect(|_, _| None, cx);
    }

    pub(crate) fn open_selected(&mut self, cx: &mut Context<Self>) {
        let ids = self
            .selection()
            .filter(|(listing, _)| self.listing_id() == Some(*listing))
            .map(|(_, ids)| ids)
            .unwrap_or_default();
        self.open_entries(&ids, cx);
    }

    pub(crate) fn open_entries(&mut self, ids: &[EntryId], cx: &mut Context<Self>) {
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
            let path = listing.path_of(file);
            if self.parts.open_entry(file, &path, cx) {
                continue;
            }
            connection
                .send(Command::OpenFile {
                    path,
                    activation: None,
                })
                .ok();
        }
    }

    pub(crate) fn selection_for(&self, next: &Listing, same_place: bool) -> Option<Selection> {
        let shown = self.listing.as_ref()?;
        if same_place {
            let selection = self
                .selection
                .as_ref()
                .filter(|selection| selection.listing == shown.id)?;
            return if shown.folders.is_some() {
                let key_before =
                    |id| entry_at(&shown.entries, id).map(|entry| shown.path_of(entry));
                selection.carried_by(next.id, key_before, &next.entries, |entry| {
                    next.path_of(entry)
                })
            } else {
                selection.carried(&shown.entries, next.id, &next.entries)
            };
        }
        if shown.folders.is_some() {
            let reveal = self.search.reveal.as_deref()?;
            return (reveal.parent() == Some(next.path.as_path()))
                .then(|| Selection::named(next.id, &next.entries, reveal.file_name()?))
                .flatten();
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

    pub fn rows_on_screen(&self) -> usize {
        self.rows_drawn.len()
    }

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
