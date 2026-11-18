//! A window opened to show some files, as an app's "Show in folder" asks:
//! the names wait for the first listing, and if it is of the folder they
//! were asked for, it selects those there and brings the first of them into
//! view. Names that aren't in the listing select nothing. Any other first
//! listing, such as the home folder after a fallback, uses them up without
//! selecting anything.

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use nova_files_api::Listing;

use super::MainWindow;
use crate::selection::Selection;

/// The names to select, and the folder they are in.
#[derive(Debug, Default)]
pub(super) struct Showing {
    folder: PathBuf,
    names: Vec<OsString>,
}

impl MainWindow {
    /// Selects `names` in `folder` once the window has listed it, unless it
    /// lists another folder first.
    pub fn select_on_arrival(&mut self, folder: PathBuf, names: Vec<OsString>) {
        self.showing = (!names.is_empty()).then_some(Showing { folder, names });
    }

    /// The selection the first listing starts with, from the names a
    /// "Show in folder" asked for; they are used up either way.
    pub(super) fn shown_selection(&mut self, listing: &Listing) -> Option<Selection> {
        let showing = self.showing.take()?;
        if showing.folder != listing.path {
            return None;
        }
        let names: HashSet<&OsStr> = showing.names.iter().map(OsString::as_os_str).collect();
        Selection::named_all(listing.id, &listing.entries, &names)
    }
}
