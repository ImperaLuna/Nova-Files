use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::hash::Hash;

use gpui::Modifiers;
use nova_files_api::{Entry, EntryId, ListingId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    Only,
    Toggle,
    Range,
}

impl Click {
    pub const fn of(modifiers: Modifiers, several: bool) -> Self {
        if !several {
            Self::Only
        } else if modifiers.shift {
            Self::Range
        } else if modifiers.control {
            Self::Toggle
        } else {
            Self::Only
        }
    }
}

pub use nova_files_shell::Move;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub listing: ListingId,
    ids: Vec<EntryId>,
    anchor: EntryId,
    cursor: EntryId,
}

impl Selection {
    pub fn only(listing: ListingId, id: EntryId) -> Self {
        Self {
            listing,
            ids: vec![id],
            anchor: id,
            cursor: id,
        }
    }

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

    pub fn ids(&self) -> &[EntryId] {
        &self.ids
    }

    pub fn contains(&self, id: EntryId) -> bool {
        self.ids.binary_search(&id).is_ok()
    }

    pub const fn cursor(&self) -> EntryId {
        self.cursor
    }

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

    #[must_use]
    pub fn banded(
        before: Option<&Self>,
        listing: ListingId,
        ids: impl IntoIterator<Item = EntryId>,
        (from, to): (EntryId, EntryId),
        add: bool,
    ) -> Self {
        let mut ids: Vec<EntryId> = ids.into_iter().collect();
        if let Some(before) = before.filter(|before| add && before.listing == listing) {
            ids.extend_from_slice(&before.ids);
        }
        ids.sort_unstable();
        ids.dedup();
        Self {
            listing,
            ids,
            anchor: from,
            cursor: to,
        }
    }

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

    pub fn carried(&self, before: &[Entry], listing: ListingId, after: &[Entry]) -> Option<Self> {
        let key_before = |id: EntryId| entry_at(before, id).map(name_of);
        self.carried_by(listing, key_before, after, name_of)
    }

    pub fn carried_by<'a, K: Eq + Hash>(
        &self,
        listing: ListingId,
        key_before: impl Fn(EntryId) -> Option<K>,
        after: &'a [Entry],
        key_after: impl Fn(&'a Entry) -> K,
    ) -> Option<Self> {
        let last = u32::try_from(after.len().checked_sub(1)?).ok()?;
        let by_key: HashMap<K, EntryId> = after
            .iter()
            .map(|entry| (key_after(entry), entry.id))
            .collect();
        let carry = |id: EntryId| key_before(id).and_then(|key| by_key.get(&key).copied());
        let mut ids: Vec<EntryId> = self.ids.iter().filter_map(|&id| carry(id)).collect();
        ids.sort_unstable();
        let cursor = carry(self.cursor).unwrap_or_else(|| EntryId(self.cursor.0.min(last)));
        let anchor = carry(self.anchor).unwrap_or(cursor);
        Some(Self {
            listing,
            ids,
            anchor,
            cursor,
        })
    }

    #[must_use]
    pub fn at_most_one(mut self) -> Self {
        let cursor = self.cursor;
        let nearest = self
            .ids
            .iter()
            .copied()
            .min_by_key(|id| id.0.abs_diff(cursor.0));
        if let Some(keep) = nearest {
            self.ids = vec![keep];
            self.anchor = keep;
        }
        self
    }

    pub fn named(listing: ListingId, entries: &[Entry], name: &OsStr) -> Option<Self> {
        find(entries, name).map(|id| Self::only(listing, id))
    }

    pub fn named_all(
        listing: ListingId,
        entries: &[Entry],
        names: &HashSet<&OsStr>,
    ) -> Option<Self> {
        let ids: Vec<EntryId> = entries
            .iter()
            .filter(|entry| names.contains(entry.name.as_os_str()))
            .map(|entry| entry.id)
            .collect();
        let first = *ids.first()?;
        Some(Self {
            listing,
            ids,
            anchor: first,
            cursor: first,
        })
    }
}

pub fn entry_at(entries: &[Entry], id: EntryId) -> Option<&Entry> {
    usize::try_from(id.0).ok().and_then(|at| entries.get(at))
}

fn name_of(entry: &Entry) -> &OsStr {
    entry.name.as_os_str()
}

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
        assert_eq!(ids(&down), [2, 4, 5]);
        assert_eq!(down.cursor(), EntryId(5));
        let up = click(down, 0, Click::Range);
        assert_eq!(ids(&up), [0, 1, 2]);
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
        assert_eq!(Move::Down.to(Some(2), 5, (3, 1)), Some(3));
        assert_eq!(Move::Down.to(Some(4), 5, (3, 1)), Some(4));
        assert_eq!(Move::Up.to(Some(0), 5, (3, 1)), Some(0));
        assert_eq!(Move::Up.to(Some(3), 5, (3, 1)), Some(2));
        assert_eq!(Move::PageDown.to(Some(1), 5, (3, 1)), Some(4));
        assert_eq!(Move::PageDown.to(Some(0), 10, (3, 1)), Some(3));
        assert_eq!(Move::PageUp.to(Some(4), 10, (3, 1)), Some(1));
        assert_eq!(Move::PageUp.to(Some(2), 10, (3, 1)), Some(0));
        assert_eq!(Move::PageDown.to(Some(1), 10, (0, 1)), Some(2));
        assert_eq!(Move::First.to(Some(4), 5, (3, 1)), Some(0));
        assert_eq!(Move::Last.to(Some(0), 5, (3, 1)), Some(4));
        assert_eq!(Move::Down.to(None, 5, (3, 1)), Some(0));
        assert_eq!(Move::PageDown.to(None, 5, (3, 1)), Some(0));
        assert_eq!(Move::Up.to(None, 5, (3, 1)), Some(4));
        assert_eq!(Move::PageUp.to(None, 5, (3, 1)), Some(4));
        assert_eq!(Move::Down.to(None, 0, (3, 1)), None);
    }

    #[test]
    fn in_icons_up_and_down_move_a_line_and_left_and_right_an_entry() {
        let to = |step: Move, from| step.to(Some(from), 11, (8, 4));
        assert_eq!(to(Move::Down, 1), Some(5));
        assert_eq!(to(Move::Up, 5), Some(1));
        assert_eq!(to(Move::Up, 3), Some(3));
        assert_eq!(to(Move::Down, 9), Some(9));
        assert_eq!(to(Move::Down, 7), Some(10));
        assert_eq!(to(Move::Down, 6), Some(10));
        assert_eq!(to(Move::Down, 5), Some(9));
        assert_eq!(to(Move::Left, 4), Some(3));
        assert_eq!(to(Move::Left, 0), Some(0));
        assert_eq!(to(Move::Right, 3), Some(4));
        assert_eq!(to(Move::Right, 10), Some(10));
        assert_eq!(Move::Right.to(None, 11, (8, 4)), Some(0));
        assert_eq!(Move::PageUp.to(Some(10), 11, (7, 4)), Some(6));
        assert_eq!(Move::PageDown.to(Some(1), 11, (7, 4)), Some(5));
        assert_eq!(Move::PageDown.to(Some(1), 11, (2, 4)), Some(5));
        assert_eq!(Move::Left.to(None, 11, (8, 4)), Some(10));
        assert_eq!(Move::Down.to(Some(0), 3, (8, 0)), Some(1));
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
    fn a_fresh_listing_keeps_the_anchor_by_name() {
        let before = entries(&[b"a", b"b", b"c"]);
        let after = entries(&[b"0", b"a", b"b", b"c"]);
        let range = Selection::clicked(
            Some(Selection::only(SHOWN, EntryId(0))),
            SHOWN,
            EntryId(1),
            Click::Range,
            |_| true,
        );
        let carried = range.carried(&before, NEXT, &after).unwrap();
        let extended = Selection::moved(Some(carried), NEXT, EntryId(3), true, |_| true);
        assert_eq!(ids(&extended), [1, 2, 3]);
    }

    #[test]
    fn at_most_one_keeps_the_nearest_selected_when_the_cursor_is_not() {
        let covered = || [2, 0, 1].map(EntryId);
        let band = Selection::banded(None, SHOWN, covered(), (EntryId(0), EntryId(3)), false);
        assert_eq!(ids(&band), [0, 1, 2]);
        assert_eq!(ids(&band.at_most_one()), [2]);
        let up = Selection::banded(None, SHOWN, covered(), (EntryId(3), EntryId(0)), false);
        assert_eq!(ids(&up.at_most_one()), [0]);
    }

    #[test]
    fn at_most_one_keeps_the_cursor_of_several() {
        let several = Selection::clicked(
            Some(Selection::only(SHOWN, EntryId(0))),
            SHOWN,
            EntryId(2),
            Click::Range,
            |_| true,
        );
        let one = several.at_most_one();
        assert_eq!(ids(&one), [2]);
        let moved = Selection::moved(Some(one.clone()), SHOWN, EntryId(3), true, |_| true);
        assert_eq!(ids(&moved), [2, 3]);
        assert_eq!(one.clone().at_most_one(), one);
        let mut none = one;
        none.toggle_cursor();
        assert_eq!(none.clone().at_most_one(), none);
        let mut away = Selection::clicked(
            Some(Selection::only(SHOWN, EntryId(0))),
            SHOWN,
            EntryId(2),
            Click::Toggle,
            |_| true,
        );
        away.toggle_cursor();
        assert_eq!(ids(&away.clone().at_most_one()), [0]);
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
    fn named_all_selects_the_names_there_with_the_cursor_on_the_first() {
        let shown = entries(&[b"a", b"b", b"c"]);
        let names = HashSet::from([OsStr::new("c"), OsStr::new("gone"), OsStr::new("a")]);
        let selection = Selection::named_all(SHOWN, &shown, &names).unwrap();
        assert_eq!(selection.ids(), [EntryId(0), EntryId(2)]);
        assert_eq!(selection.cursor(), EntryId(0));
        let missing = HashSet::from([OsStr::new("x")]);
        assert_eq!(Selection::named_all(SHOWN, &shown, &missing), None);
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
