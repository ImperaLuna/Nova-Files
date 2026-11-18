use std::ffi::OsString;

use gpui::Context;
use nova_files_api::{EntryId, EntryKind, ListingId};

use crate::selection::{Click, Selection, entry_at};
use crate::window::MainWindow;

impl MainWindow {
    pub(super) fn select(
        &mut self,
        listing: ListingId,
        id: EntryId,
        click: Click,
        cx: &mut Context<Self>,
    ) {
        self.reselect(
            |before, picks| Some(Selection::clicked(before, listing, id, click, picks)),
            cx,
        );
    }

    pub(crate) fn pick_selection_changed(&mut self, cx: &mut Context<Self>) {
        let Some(pick) = &mut self.pick else { return };
        if pick.mode == super::Mode::Folders {
            return;
        }
        let entries = self
            .listing
            .as_ref()
            .map(|shown| &*shown.entries)
            .unwrap_or_default();
        let selection = self.selection.as_ref().filter(|selection| {
            self.listing.as_ref().map(|shown| shown.id) == Some(selection.listing)
        });
        let ids = selection.map(Selection::ids).unwrap_or_default();
        let files: Vec<(EntryId, String)> = ids
            .iter()
            .filter_map(|&id| entry_at(entries, id))
            .filter(|entry| entry.kind == EntryKind::File)
            .map(|entry| (entry.id, entry.name.to_string_lossy().into_owned()))
            .collect();
        if files.is_empty() && !ids.is_empty() {
            return;
        }
        let names: Vec<String> = files.iter().map(|(_, name)| name.clone()).collect();
        let text = names_text(&names);
        pick.name.update(cx, |field, cx| field.set_text(&text, cx));
        pick.copied =
            (!files.is_empty()).then(|| (files.into_iter().map(|(id, _)| id).collect(), text));
        pick.conflict = None;
        pick.recheck = None;
        pick.jump = None;
    }

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
