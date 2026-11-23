use gpui::{ClickEvent, Context, KeyDownEvent, Modifiers, Window};
use nova_files_api::{Entry, EntryId, EntryKind, Listing, ListingId};
use nova_files_shell::{self as shell, ClickAnswer, Entries};

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
        self.reselect_in_place(change, cx);
        if let Some(cursor) = self.cursor() {
            self.scroll_to_entry(cursor, cx);
        }
    }

    fn reselect_in_place(
        &mut self,
        change: impl FnOnce(Option<Selection>, &dyn Fn(EntryId) -> bool) -> Option<Selection>,
        cx: &mut Context<Self>,
    ) {
        let before = self.selection.take();
        let entries = self
            .listing
            .as_ref()
            .map(|listing| &*listing.entries)
            .unwrap_or_default();
        let parts = &self.parts;
        let picks = |id: EntryId| {
            let entry = entry_at(entries, id);
            entry.is_none_or(|entry| parts.selectable(entry, cx))
        };
        let several = self.parts.several(cx);
        self.selection = change(before, &picks).map(|selection| {
            if several {
                selection
            } else {
                selection.at_most_one()
            }
        });
        self.publish(cx);
        self.host().entries.update(cx, Entries::selection_made);
        cx.notify();
    }

    pub(crate) fn entry_pressed(
        &mut self,
        (listing, id): (ListingId, EntryId),
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        if self.picker() || self.selects((listing, id)) {
            return;
        }
        self.entry_chosen((listing, id), modifiers, false, cx);
        self.selected_on_press = Some((listing, id));
    }

    pub fn entry_clicked(
        &mut self,
        at: (ListingId, EntryId),
        event: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let double = event.click_count() == DOUBLE_CLICK;
        if !double && self.selected_on_press.take() == Some(at) {
            return;
        }
        if event.click_count() <= DOUBLE_CLICK {
            self.entry_chosen(at, event.modifiers(), double, cx);
        }
    }

    fn entry_chosen(
        &mut self,
        (listing, id): (ListingId, EntryId),
        modifiers: Modifiers,
        double: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.entry(listing, id).cloned() else {
            return;
        };
        let click = Click::of(modifiers, self.parts.several(cx));
        let asked = shell::Click {
            only: click == Click::Only,
            double,
        };
        match self.parts.clicked(listing, &entry, asked, cx) {
            None if double => self.open_entries(&[id], cx),
            None => self.reselect_in_place(
                |before, picks| {
                    let click = Click::of(modifiers, true);
                    Some(Selection::clicked(before, listing, id, click, picks))
                },
                cx,
            ),
            Some(ClickAnswer::Open) => self.open_entry(id),
            Some(ClickAnswer::Select) => self.reselect_in_place(
                |before, picks| Some(Selection::clicked(before, listing, id, click, picks)),
                cx,
            ),
            Some(ClickAnswer::Done) => {}
        }
        cx.notify();
    }

    fn entry(&self, listing: ListingId, id: EntryId) -> Option<&Entry> {
        let shown = self.listing.as_ref().filter(|shown| shown.id == listing)?;
        entry_at(&shown.entries, id)
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
        let per_line = self.grid(cx).per_line;
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

    pub(crate) fn jump_typed(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let held = event.keystroke.modifiers;
        if self.picker() || held.control || held.alt || held.platform || held.function {
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
        let (prefix, past) = self.jump.typed(typed, now);
        self.host()
            .entries
            .update(cx, |entries, _| entries.set_jumped(Some(now)));
        let typed = jump::Jump { prefix, past };
        let Some(listing) = &self.listing else {
            let taken = self.parts.typed(&typed, &self.scene(cx), cx);
            if !taken {
                self.jump = jump::TypeJump::default();
                self.host()
                    .entries
                    .update(cx, |entries, _| entries.set_jumped(None));
            }
            return taken;
        };
        let names = listing.entries.iter().map(|entry| entry.name.as_os_str());
        let found = typed.find(names, self.cursor());
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
        if self.connection.is_none() {
            return;
        }
        for file in entries.iter().filter(|entry| entry.kind == EntryKind::File) {
            self.parts.open_entry(file, &listing.path_of(file), cx);
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
