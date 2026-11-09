//! What a picker has selected: one entry, or several when the app asks for
//! more than one.
//!
//! A click selects one entry. When the app allows several, Ctrl+click adds
//! an entry or takes it out, and Shift+click selects every entry from the
//! one clicked before to the one clicked now (the anchor stays, so another
//! Shift+click redraws the range from it). A range holds only what the
//! picker picks: files in an Open dialog for files, folders in one for
//! folders.

use std::ffi::OsString;

use gpui::{Context, Modifiers};
use nova_files_api::{Entry, EntryId, EntryKind, ListingId};

use super::Mode;
use crate::window::MainWindow;

/// How a click changes the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    /// Selects only the entry clicked.
    Only,
    /// Ctrl+click: adds the entry clicked or takes it out.
    Toggle,
    /// Shift+click: selects from the anchor to the entry clicked.
    Range,
}

impl Click {
    /// What a click with `modifiers` does, in a picker that allows several
    /// entries when `several`; one that doesn't ignores the keys held.
    pub fn of(modifiers: Modifiers, several: bool) -> Self {
        if !several {
            Self::Only
        } else if modifiers.shift {
            Self::Range
        } else if modifiers.secondary() {
            Self::Toggle
        } else {
            Self::Only
        }
    }
}

/// The entries selected in one listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// The listing the entries belong to.
    pub listing: ListingId,
    /// The entries, in the order the listing shows them.
    ids: Vec<EntryId>,
    /// The entry a Shift+click selects from.
    anchor: EntryId,
}

impl Selection {
    /// Just `id` of `listing`.
    pub fn only(listing: ListingId, id: EntryId) -> Self {
        Self {
            listing,
            ids: vec![id],
            anchor: id,
        }
    }

    /// The entries selected, in the order the listing shows them.
    pub fn ids(&self) -> &[EntryId] {
        &self.ids
    }

    /// Whether `id` is selected.
    pub fn contains(&self, id: EntryId) -> bool {
        self.ids.binary_search(&id).is_ok()
    }

    /// `click` on `id` of `listing`. A click in another listing than this
    /// one's starts over there. `picks` says which entries a range holds.
    #[must_use]
    pub fn clicked(
        selection: Option<Self>,
        listing: ListingId,
        id: EntryId,
        click: Click,
        picks: impl Fn(EntryId) -> bool,
    ) -> Self {
        let Some(mut selection) = selection.filter(|selection| selection.listing == listing) else {
            return Self::only(listing, id);
        };
        match click {
            Click::Only => return Self::only(listing, id),
            Click::Toggle => match selection.ids.binary_search(&id) {
                Ok(at) => {
                    selection.ids.remove(at);
                }
                Err(at) => selection.ids.insert(at, id),
            },
            Click::Range => {
                let (from, to) = if selection.anchor <= id {
                    (selection.anchor.0, id.0)
                } else {
                    (id.0, selection.anchor.0)
                };
                selection.ids = (from..=to).map(EntryId).filter(|&id| picks(id)).collect();
                return selection;
            }
        }
        selection.anchor = id;
        selection
    }
}

impl MainWindow {
    /// `click` on `id` of `listing` in a picker: selects as the click says
    /// and, in a picker for files, puts the names of those selected in the
    /// name box, a new name as when typed.
    pub(super) fn select(
        &mut self,
        listing: ListingId,
        id: EntryId,
        click: Click,
        cx: &mut Context<Self>,
    ) {
        let Some(pick) = &mut self.pick else { return };
        let entries = self
            .listing
            .as_ref()
            .filter(|shown| shown.id == listing)
            .map(|shown| &*shown.entries)
            .unwrap_or_default();
        let wanted = if pick.mode == Mode::Folders {
            EntryKind::Folder
        } else {
            EntryKind::File
        };
        let selection = Selection::clicked(pick.selected.take(), listing, id, click, |id| {
            entry_at(entries, id).is_some_and(|entry| entry.kind == wanted)
        });
        if pick.mode != Mode::Folders {
            let names: Vec<String> = selection
                .ids()
                .iter()
                .filter_map(|&id| entry_at(entries, id))
                .map(|entry| entry.name.to_string_lossy().into_owned())
                .collect();
            let text = names_text(&names);
            pick.name.update(cx, |field, cx| field.set_text(&text, cx));
            pick.copied = (!names.is_empty()).then(|| (selection.ids().to_vec(), text));
            // No question, and no save or jump still waiting on the old
            // name.
            pick.conflict = None;
            pick.recheck = None;
            pick.jump = None;
        }
        pick.selected = Some(selection);
    }

    /// The entries `ids` of the listing on screen that it still has: each
    /// one's kind, id and name.
    pub(super) fn shown_entries(&self, ids: &[EntryId]) -> Vec<(EntryKind, EntryId, OsString)> {
        let entries = self
            .listing
            .as_ref()
            .map(|shown| &*shown.entries)
            .unwrap_or_default();
        ids.iter()
            .filter_map(|&id| entry_at(entries, id))
            .map(|entry| (entry.kind, entry.id, entry.name.clone()))
            .collect()
    }
}

/// The entry `id` of a listing's `entries`.
fn entry_at(entries: &[Entry], id: EntryId) -> Option<&Entry> {
    usize::try_from(id.0).ok().and_then(|at| entries.get(at))
}

/// What the name box shows for the names of the files selected: the name
/// of one, each of several in quotes, nothing for none.
pub fn names_text(names: &[String]) -> String {
    match names {
        [one] => one.clone(),
        names => names
            .iter()
            .map(|name| format!("\"{name}\""))
            .collect::<Vec<_>>()
            .join(" "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOWN: ListingId = ListingId(1);

    fn ids(selection: &Selection) -> Vec<u32> {
        selection.ids().iter().map(|id| id.0).collect()
    }

    fn click(selection: Selection, id: u32, click: Click) -> Selection {
        Selection::clicked(Some(selection), SHOWN, EntryId(id), click, |id| id.0 != 3)
    }

    #[test]
    fn keys_held_count_only_where_the_app_allows_several() {
        let shift = Modifiers::shift();
        let ctrl = Modifiers::control();
        assert_eq!(Click::of(shift, true), Click::Range);
        assert_eq!(Click::of(ctrl, true), Click::Toggle);
        assert_eq!(Click::of(Modifiers::none(), true), Click::Only);
        assert_eq!(Click::of(shift, false), Click::Only);
        assert_eq!(Click::of(ctrl, false), Click::Only);
        let both = Modifiers {
            shift: true,
            control: true,
            ..Modifiers::none()
        };
        assert_eq!(Click::of(both, true), Click::Range);
    }

    #[test]
    fn ctrl_click_adds_an_entry_or_takes_it_out_in_listing_order() {
        let one = Selection::only(SHOWN, EntryId(4));
        let two = click(one, 1, Click::Toggle);
        assert_eq!(ids(&two), [1, 4]);
        assert!(two.contains(EntryId(1)) && two.contains(EntryId(4)));
        assert!(!two.contains(EntryId(2)));
        let back = click(two, 4, Click::Toggle);
        assert_eq!(ids(&back), [1]);
        let none = click(back, 1, Click::Toggle);
        assert_eq!(none.ids(), []);
    }

    #[test]
    fn shift_click_selects_from_the_anchor_and_keeps_it() {
        let start = Selection::only(SHOWN, EntryId(2));
        let down = click(start, 5, Click::Range);
        // 3 isn't something this picker picks.
        assert_eq!(ids(&down), [2, 4, 5]);
        let up = click(down, 0, Click::Range);
        assert_eq!(ids(&up), [0, 1, 2]);
        // Ctrl+click moves the anchor a range starts from.
        let moved = click(click(up, 5, Click::Toggle), 6, Click::Range);
        assert_eq!(ids(&moved), [5, 6]);
    }

    #[test]
    fn a_plain_click_or_another_listing_starts_over() {
        let two = click(Selection::only(SHOWN, EntryId(1)), 2, Click::Toggle);
        assert_eq!(ids(&click(two.clone(), 4, Click::Only)), [4]);
        let elsewhere =
            Selection::clicked(Some(two), ListingId(2), EntryId(0), Click::Range, |_| true);
        assert_eq!(elsewhere, Selection::only(ListingId(2), EntryId(0)));
        let first = Selection::clicked(None, SHOWN, EntryId(3), Click::Toggle, |_| true);
        assert_eq!(first, Selection::only(SHOWN, EntryId(3)));
    }

    #[test]
    fn the_box_shows_one_name_as_it_is_and_several_in_quotes() {
        let names = |names: &[&str]| {
            names
                .iter()
                .map(|&name| name.to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(names_text(&names(&["a b.txt"])), "a b.txt");
        assert_eq!(
            names_text(&names(&["a.txt", "b.txt"])),
            "\"a.txt\" \"b.txt\""
        );
        assert_eq!(names_text(&[]), "");
    }
}
