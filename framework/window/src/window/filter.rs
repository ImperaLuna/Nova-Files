use std::mem;

use gpui::Context;
use nova_files_api::Listing;
use nova_files_shell::Location;

use super::MainWindow;
use crate::filtering::Filter;
use crate::selection::Selection;

impl MainWindow {
    pub(crate) const fn filter(&self) -> &Filter {
        &self.filter
    }

    pub(crate) fn filters_here(&self) -> bool {
        !self.picker()
            && !self.searching()
            && matches!(self.place_on_screen_now(), Some(Location::Folder(_)))
    }

    pub(crate) fn filter_typed(&mut self, typed: &str) -> bool {
        let total = self
            .listing
            .as_ref()
            .map_or(0, |listing| listing.entries.len());
        self.filter.typed(typed, total) && self.list_filtered()
    }

    pub(crate) fn erase_filter_letter(&mut self, cx: &mut Context<Self>) {
        if self.filter.erase_letter() {
            self.list_filtered();
            cx.notify();
        }
    }

    pub(crate) fn clear_filter(&mut self, cx: &mut Context<Self>) {
        if self.filter.clear() {
            self.list_filtered();
            cx.notify();
        }
    }

    pub(super) fn drop_filter(&mut self) -> Filter {
        mem::take(&mut self.filter)
    }

    pub(super) fn first_match(&mut self, listing: &Listing) -> Option<Selection> {
        if !mem::take(&mut self.filter.moves_cursor) {
            return None;
        }
        let first = listing.entries.first()?;
        Selection::named(listing.id, &listing.entries, &first.name)
    }

    fn list_filtered(&mut self) -> bool {
        self.place_asked_last()
            .is_some_and(|place| self.request_place(&place))
    }
}
