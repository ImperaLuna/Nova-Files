//! A window opened to show some files, as an app's "Show in folder" asks:
//! the names wait until the first listing arrives, which selects those
//! there and brings the first of them into view. Names that aren't there
//! (hidden, gone, or a fallback folder listed instead) select nothing, and
//! later listings never pick them up.

use std::ffi::OsString;

use nova_files_api::Listing;

use super::MainWindow;
use crate::selection::Selection;

impl MainWindow {
    /// Selects `names` in the folder once the window has listed it.
    pub fn select_on_arrival(&mut self, names: Vec<OsString>) {
        self.showing = names;
    }

    /// The selection the first listing starts with, from the names a
    /// "Show in folder" asked for; they are used up either way.
    pub(super) fn shown_selection(&mut self, listing: &Listing) -> Option<Selection> {
        let names = std::mem::take(&mut self.showing);
        Selection::named_all(listing.id, &listing.entries, &names)
    }
}
