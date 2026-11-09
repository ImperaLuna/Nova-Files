//! What a view has selected, and the row the keyboard is on.
//!
//! A selection belongs to one listing: the entries selected, an anchor a
//! Shift range starts from, and a cursor, the row the keyboard moves from.
//! A click selects one entry; Ctrl+click adds an entry or takes it out,
//! and Shift+click selects from the anchor to the entry clicked (the
//! anchor stays, so another Shift+click redraws the range from it). The
//! keyboard moves the cursor and selects the row it lands on, or with
//! Shift the range from the anchor to it. A range holds only the entries
//! the view picks (a picker for files picks only files).
//!
//! When the same folder is read again, [`Selection::carried`] keeps the
//! entries by name, as raw bytes, since names needn't be UTF-8; a cursor
//! whose row is gone goes to the nearest row left.

use std::ffi::OsStr;

use gpui::Modifiers;
use nova_files_api::{Entry, EntryId, ListingId};

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
    /// What a click with `modifiers` does, in a view that allows several
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

/// Where a key moves the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Up,
    Down,
    /// A screen of rows up.
    PageUp,
    /// A screen of rows down.
    PageDown,
    First,
    Last,
}

impl Move {
    /// The row a move from `from` lands on, in a list of `len` rows with
    /// `page` rows on screen. With no row yet, a move down starts at the
    /// first row and a move up at the last.
    pub fn to(self, from: Option<usize>, len: usize, page: usize) -> Option<usize> {
        let last = len.checked_sub(1)?;
        let page = page.max(1);
        Some(match (self, from) {
            (Self::First, _) | (Self::Down | Self::PageDown, None) => 0,
            (Self::Last, _) | (Self::Up | Self::PageUp, None) => last,
            (Self::Up, Some(from)) => from.saturating_sub(1),
            (Self::PageUp, Some(from)) => from.saturating_sub(page),
            (Self::Down, Some(from)) => from.saturating_add(1).min(last),
            (Self::PageDown, Some(from)) => from.saturating_add(page).min(last),
        })
    }
}

/// The entries selected in one listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// The listing the entries belong to.
    pub listing: ListingId,
    /// The entries, in the order the listing shows them.
    ids: Vec<EntryId>,
    /// The entry a Shift range starts from.
    anchor: EntryId,
    /// The row the keyboard is on.
    cursor: EntryId,
}

impl Selection {
    /// Just `id` of `listing`.
    pub fn only(listing: ListingId, id: EntryId) -> Self {
        Self {
            listing,
            ids: vec![id],
            anchor: id,
            cursor: id,
        }
    }

    /// Every entry of `listing`'s `len` that `picks` allows; the cursor
    /// stays where it was in `before`, if it was in this listing.
    pub fn all(
        before: Option<&Self>,
        listing: ListingId,
        len: u32,
        picks: impl Fn(EntryId) -> bool,
    ) -> Option<Self> {
        let ids: Vec<EntryId> = (0..len).map(EntryId).filter(|&id| picks(id)).collect();
        let first = *ids.first()?;
        let cursor = before
            .filter(|before| before.listing == listing)
            .map_or(first, |before| before.cursor);
        Some(Self {
            listing,
            ids,
            anchor: first,
            cursor,
        })
    }

    /// The entries selected, in the order the listing shows them.
    pub fn ids(&self) -> &[EntryId] {
        &self.ids
    }

    /// Whether `id` is selected.
    pub fn contains(&self, id: EntryId) -> bool {
        self.ids.binary_search(&id).is_ok()
    }

    /// The row the keyboard is on.
    pub const fn cursor(&self) -> EntryId {
        self.cursor
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
            Click::Toggle => {
                selection.toggle(id);
                selection.anchor = id;
            }
            Click::Range => selection.range_to(id, picks),
        }
        selection.cursor = id;
        selection
    }

    /// The cursor moved to `id` of `listing` by a key: selects just that
    /// entry, or with `extend` the range from the anchor to it.
    #[must_use]
    pub fn moved(
        selection: Option<Self>,
        listing: ListingId,
        id: EntryId,
        extend: bool,
        picks: impl Fn(EntryId) -> bool,
    ) -> Self {
        let click = if extend { Click::Range } else { Click::Only };
        Self::clicked(selection, listing, id, click, picks)
    }

    /// Ctrl+Space: adds the entry at the cursor or takes it out.
    pub fn toggle_cursor(&mut self) {
        self.toggle(self.cursor);
        self.anchor = self.cursor;
    }

    fn toggle(&mut self, id: EntryId) {
        match self.ids.binary_search(&id) {
            Ok(at) => {
                self.ids.remove(at);
            }
            Err(at) => self.ids.insert(at, id),
        }
    }

    fn range_to(&mut self, id: EntryId, picks: impl Fn(EntryId) -> bool) {
        let (from, to) = if self.anchor <= id {
            (self.anchor.0, id.0)
        } else {
            (id.0, self.anchor.0)
        };
        self.ids = (from..=to).map(EntryId).filter(|&id| picks(id)).collect();
    }

    /// This selection, made in `before`, carried to `after`, a fresh
    /// listing `listing` of the same folder: the same names stay selected,
    /// and a cursor whose entry is gone goes to the nearest row left.
    pub fn carried(&self, before: &[Entry], listing: ListingId, after: &[Entry]) -> Option<Self> {
        let name = |id: EntryId| entry_at(before, id).map(|entry| entry.name.as_os_str());
        let ids: Vec<EntryId> = self
            .ids
            .iter()
            .filter_map(|&id| find(after, name(id)?))
            .collect();
        let last = u32::try_from(after.len().checked_sub(1)?).ok()?;
        let cursor = name(self.cursor)
            .and_then(|name| find(after, name))
            .unwrap_or_else(|| EntryId(self.cursor.0.min(last)));
        let anchor = name(self.anchor)
            .and_then(|name| find(after, name))
            .unwrap_or(cursor);
        let mut ids = ids;
        ids.sort_unstable();
        Some(Self {
            listing,
            ids,
            anchor,
            cursor,
        })
    }

    /// Just the entry named `name` in `entries` of `listing`, if there is one.
    pub fn named(listing: ListingId, entries: &[Entry], name: &OsStr) -> Option<Self> {
        find(entries, name).map(|id| Self::only(listing, id))
    }
}

/// The entry `id` of a listing's `entries`.
pub fn entry_at(entries: &[Entry], id: EntryId) -> Option<&Entry> {
    usize::try_from(id.0).ok().and_then(|at| entries.get(at))
}

/// The id of the entry named exactly `name` in `entries`.
fn find(entries: &[Entry], name: &OsStr) -> Option<EntryId> {
    entries
        .iter()
        .find(|entry| entry.name == name)
        .map(|entry| entry.id)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt as _;

    use nova_files_api::EntryKind;

    use super::*;

    const SHOWN: ListingId = ListingId(1);
    const NEXT: ListingId = ListingId(2);

    fn ids(selection: &Selection) -> Vec<u32> {
        selection.ids().iter().map(|id| id.0).collect()
    }

    fn click(selection: Selection, id: u32, click: Click) -> Selection {
        Selection::clicked(Some(selection), SHOWN, EntryId(id), click, |id| id.0 != 3)
    }

    fn entries(names: &[&[u8]]) -> Vec<Entry> {
        names
            .iter()
            .zip(0..)
            .map(|(name, id)| Entry {
                id: EntryId(id),
                name: OsString::from_vec(name.to_vec()),
                kind: EntryKind::File,
            })
            .collect()
    }

    #[test]
    fn keys_held_count_only_where_several_are_allowed() {
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
        assert_eq!(two.cursor(), EntryId(1));
        let back = click(two, 4, Click::Toggle);
        assert_eq!(ids(&back), [1]);
        let none = click(back, 1, Click::Toggle);
        assert_eq!(none.ids(), []);
    }

    #[test]
    fn shift_click_selects_from_the_anchor_and_keeps_it() {
        let start = Selection::only(SHOWN, EntryId(2));
        let down = click(start, 5, Click::Range);
        // 3 isn't something this view picks.
        assert_eq!(ids(&down), [2, 4, 5]);
        assert_eq!(down.cursor(), EntryId(5));
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
        let elsewhere = Selection::clicked(Some(two), NEXT, EntryId(0), Click::Range, |_| true);
        assert_eq!(elsewhere, Selection::only(NEXT, EntryId(0)));
        let first = Selection::clicked(None, SHOWN, EntryId(3), Click::Toggle, |_| true);
        assert_eq!(first, Selection::only(SHOWN, EntryId(3)));
    }

    #[test]
    fn moves_stop_at_the_ends_and_start_from_an_end() {
        assert_eq!(Move::Down.to(Some(2), 5, 3), Some(3));
        assert_eq!(Move::Down.to(Some(4), 5, 3), Some(4));
        assert_eq!(Move::Up.to(Some(0), 5, 3), Some(0));
        assert_eq!(Move::Up.to(Some(3), 5, 3), Some(2));
        assert_eq!(Move::PageDown.to(Some(1), 5, 3), Some(4));
        assert_eq!(Move::PageDown.to(Some(0), 10, 3), Some(3));
        assert_eq!(Move::PageUp.to(Some(4), 10, 3), Some(1));
        assert_eq!(Move::PageUp.to(Some(2), 10, 3), Some(0));
        // An empty screen still moves a row.
        assert_eq!(Move::PageDown.to(Some(1), 10, 0), Some(2));
        assert_eq!(Move::First.to(Some(4), 5, 3), Some(0));
        assert_eq!(Move::Last.to(Some(0), 5, 3), Some(4));
        assert_eq!(Move::Down.to(None, 5, 3), Some(0));
        assert_eq!(Move::PageDown.to(None, 5, 3), Some(0));
        assert_eq!(Move::Up.to(None, 5, 3), Some(4));
        assert_eq!(Move::PageUp.to(None, 5, 3), Some(4));
        assert_eq!(Move::Down.to(None, 0, 3), None);
    }

    #[test]
    fn keys_select_the_row_or_extend_from_the_anchor() {
        let start = Selection::only(SHOWN, EntryId(2));
        let down = Selection::moved(Some(start), SHOWN, EntryId(4), true, |_| true);
        assert_eq!(ids(&down), [2, 3, 4]);
        let on = Selection::moved(Some(down), SHOWN, EntryId(5), false, |_| true);
        assert_eq!(ids(&on), [5]);
        assert_eq!(on.cursor(), EntryId(5));
    }

    #[test]
    fn ctrl_space_toggles_the_cursor_row() {
        let mut selection = Selection::only(SHOWN, EntryId(2));
        selection.toggle_cursor();
        assert_eq!(selection.ids(), []);
        assert_eq!(selection.cursor(), EntryId(2));
        selection.toggle_cursor();
        assert_eq!(ids(&selection), [2]);
    }

    #[test]
    fn all_selects_what_the_view_picks_and_keeps_the_cursor() {
        let before = Selection::only(SHOWN, EntryId(2));
        let all = Selection::all(Some(&before), SHOWN, 5, |id| id.0 != 3).unwrap();
        assert_eq!(ids(&all), [0, 1, 2, 4]);
        assert_eq!(all.cursor(), EntryId(2));
        let fresh = Selection::all(Some(&before), NEXT, 2, |_| true).unwrap();
        assert_eq!(fresh.cursor(), EntryId(0));
        assert_eq!(Selection::all(None, SHOWN, 0, |_| true), None);
        assert_eq!(Selection::all(None, SHOWN, 3, |_| false), None);
    }

    #[test]
    fn a_fresh_listing_keeps_the_names_selected() {
        let before = entries(&[b"a", b"b", b"c\xff", b"d"]);
        let after = entries(&[b"0", b"a", b"c\xff", b"d"]);
        let selection = Selection::clicked(
            Some(Selection::only(SHOWN, EntryId(2))),
            SHOWN,
            EntryId(3),
            Click::Toggle,
            |_| true,
        );
        let carried = selection.carried(&before, NEXT, &after).unwrap();
        assert_eq!(carried.listing, NEXT);
        assert_eq!(ids(&carried), [2, 3]);
        assert_eq!(carried.cursor(), EntryId(3));
    }

    #[test]
    fn a_cursor_whose_entry_is_gone_goes_to_the_nearest_row() {
        let before = entries(&[b"a", b"b", b"c", b"d"]);
        let selection = Selection::only(SHOWN, EntryId(3));
        let after = entries(&[b"a", b"b"]);
        let carried = selection.carried(&before, NEXT, &after).unwrap();
        assert_eq!(carried.ids(), []);
        assert_eq!(carried.cursor(), EntryId(1));
        let same_row = Selection::only(SHOWN, EntryId(1))
            .carried(&before, NEXT, &entries(&[b"a", b"x", b"y"]))
            .unwrap();
        assert_eq!(same_row.cursor(), EntryId(1));
        assert_eq!(selection.carried(&before, NEXT, &[]), None);
    }

    #[test]
    fn named_selects_the_exact_name() {
        let shown = entries(&[b"a", b"b\xff"]);
        let raw = OsString::from_vec(b"b\xff".to_vec());
        assert_eq!(
            Selection::named(SHOWN, &shown, &raw),
            Some(Selection::only(SHOWN, EntryId(1)))
        );
        assert_eq!(Selection::named(SHOWN, &shown, OsStr::new("B")), None);
    }
}
