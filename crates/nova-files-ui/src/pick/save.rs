//! Saving: where a save lands, and what happens when a file is there.
//!
//! The folder on screen can be out of date by the time the user saves:
//! another program may have made a file with the same name since it was
//! listed. So before answering, the window asks the core to list the
//! folder again, and answers only from that fresh listing. A name that is
//! taken by then asks Replace or Keep both, with a free numbered name
//! worked out from the same listing. Replace answers straight away, since
//! the user chose to save over the file.
//!
//! Names count as taken when they differ only in case: on a FAT drive
//! `report.txt` is `REPORT.TXT`, and the window can't tell which file
//! system a folder is on, so it asks rather than risk saving over a file.

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use gpui::{Context, Window};

use nova_files_api::ListingId;

use super::{Conflict, typed};
use crate::selection::Selection;
use crate::window::MainWindow;

/// A save waiting for a fresh listing of `folder`.
pub struct Recheck {
    folder: PathBuf,
    /// The listing it asked for.
    pub listing: Option<ListingId>,
    /// The name to save under, if it is still free.
    name: OsString,
    /// For Keep both, the file it keeps: the one to ask about again.
    beside: Option<OsString>,
}

impl MainWindow {
    /// Saves `name` in `folder` once a fresh listing shows it is free.
    pub fn save_new(&mut self, folder: &Path, name: OsString) {
        self.recheck(folder, name, None);
    }

    fn recheck(&mut self, folder: &Path, name: OsString, beside: Option<OsString>) {
        self.open(folder);
        let listing = self.asked;
        if let Some(pick) = &mut self.pick {
            pick.recheck = Some(Recheck {
                folder: folder.to_path_buf(),
                listing,
                name,
                beside,
            });
        }
    }

    /// Asks Replace or Keep both about the file `name` in `folder`, with
    /// that file selected.
    pub fn ask_over(&mut self, folder: &Path, name: OsString) {
        let names: HashSet<OsString> = self
            .listing
            .iter()
            .flat_map(|listing| listing.entries.iter())
            .map(|entry| folded(&entry.name))
            .collect();
        let keep_both = typed::keep_both(&name, |candidate| names.contains(&folded(candidate)));
        // The file in the way shows as selected: that is the question.
        let there = self.listing.as_ref().and_then(|listing| {
            let entry = listing
                .entries
                .iter()
                .find(|entry| entry.name == name)
                .or_else(|| {
                    let name = folded(&name);
                    listing
                        .entries
                        .iter()
                        .find(|entry| folded(&entry.name) == name)
                })?;
            Some((listing.id, entry.id))
        });
        self.selection = there.map(|(listing, id)| Selection::only(listing, id));
        if let Some(pick) = &mut self.pick {
            pick.conflict = Some(Conflict {
                path: folder.join(name),
                keep_both,
            });
        }
    }

    /// The fresh listing a save waited for is on screen: answers if its
    /// name is still free, and closes the window; otherwise asks.
    pub fn save_checked(&mut self, recheck: Recheck, cx: &mut Context<Self>) {
        let name = folded(&recheck.name);
        let taken = self.listing.as_ref().is_some_and(|listing| {
            listing
                .entries
                .iter()
                .any(|entry| folded(&entry.name) == name)
        });
        if taken {
            let over = recheck.beside.unwrap_or(recheck.name);
            self.ask_over(&recheck.folder, over);
            return;
        }
        let Some(handle) = self.pick.as_ref().map(|pick| pick.window) else {
            return;
        };
        if self.answer(recheck.folder.join(recheck.name)) {
            // Closed once this event is handled, outside the view's update.
            cx.defer(move |cx| {
                handle
                    .update(cx, |_, window, _| window.remove_window())
                    .ok();
            });
        }
    }

    /// Replace: saves over the file that is there.
    pub fn replace(&mut self, window: &mut Window) {
        let Some(path) = self
            .pick
            .as_mut()
            .and_then(|pick| pick.conflict.take())
            .map(|conflict| conflict.path)
        else {
            return;
        };
        if self.answer(path) {
            window.remove_window();
        }
    }

    /// Keep both: saves beside the file that is there, under the numbered
    /// name, once a fresh listing shows that name is still free.
    pub fn keep_both(&mut self) {
        let Some(conflict) = self.pick.as_mut().and_then(|pick| pick.conflict.take()) else {
            return;
        };
        let (Some(folder), Some(beside)) = (conflict.path.parent(), conflict.path.file_name())
        else {
            return;
        };
        self.recheck(folder, conflict.keep_both, Some(beside.to_owned()));
    }
}

/// `name` as names are compared for a save: ignoring case. A name that
/// isn't UTF-8 is compared by its bytes.
fn folded(name: &OsStr) -> OsString {
    name.to_str()
        .map_or_else(|| name.to_owned(), |text| text.to_lowercase().into())
}
