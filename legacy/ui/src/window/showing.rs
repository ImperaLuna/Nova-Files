use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use nova_files_api::Listing;

use super::MainWindow;
use crate::selection::Selection;

#[derive(Debug, Default)]
pub(super) struct Showing {
    folder: PathBuf,
    names: Vec<OsString>,
}

impl MainWindow {
    pub fn select_on_arrival(&mut self, folder: PathBuf, names: Vec<OsString>) {
        self.showing = (!names.is_empty()).then_some(Showing { folder, names });
    }

    pub(super) fn shown_selection(&mut self, listing: &Listing) -> Option<Selection> {
        let showing = self.showing.take()?;
        if showing.folder != listing.path {
            return None;
        }
        let names: HashSet<&OsStr> = showing.names.iter().map(OsString::as_os_str).collect();
        let shown = Selection::named_all(listing.id, &listing.entries, &names);
        if shown.is_none() {
            self.showing = Some(showing);
        }
        shown
    }
}
