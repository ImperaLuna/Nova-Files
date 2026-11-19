use std::ffi::OsString;
use std::path::PathBuf;

use gpui::{Context, EventEmitter};
use nova_files_api::{Entry, EntryId, Listing, ListingId};

use crate::location::Location;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Shown {
    pub location: Option<Location>,
    pub listing: Option<Listing>,
    pub selected: Vec<EntryId>,
    pub picker: bool,
    pub asked: Option<ListingId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntriesAsked;

#[derive(Debug, Default)]
pub struct Entries {
    shown: Shown,
    selects: Option<Vec<OsString>>,
    selects_later: Option<(PathBuf, Vec<OsString>)>,
}

impl EventEmitter<EntriesAsked> for Entries {}

impl Entries {
    pub const fn location(&self) -> Option<&Location> {
        self.shown.location.as_ref()
    }

    pub const fn listing(&self) -> Option<&Listing> {
        self.shown.listing.as_ref()
    }

    pub const fn picker(&self) -> bool {
        self.shown.picker
    }

    pub const fn asked(&self) -> Option<ListingId> {
        self.shown.asked
    }

    pub const fn selected_count(&self) -> usize {
        self.shown.selected.len()
    }

    pub fn selected(&self) -> Vec<&Entry> {
        let Some(listing) = &self.shown.listing else {
            return Vec::new();
        };
        self.shown
            .selected
            .iter()
            .filter_map(|id| {
                usize::try_from(id.0)
                    .ok()
                    .and_then(|at| listing.entries.get(at))
            })
            .collect()
    }

    pub fn selected_paths(&self) -> Vec<PathBuf> {
        let Some(listing) = &self.shown.listing else {
            return Vec::new();
        };
        self.selected()
            .into_iter()
            .map(|entry| listing.path_of(entry))
            .collect()
    }

    pub fn show(&mut self, shown: Shown, cx: &mut Context<Self>) {
        if self.shown != shown {
            self.shown = shown;
            cx.notify();
        }
    }

    pub fn select(&mut self, names: Vec<OsString>, cx: &mut Context<Self>) {
        self.selects = Some(names);
        cx.emit(EntriesAsked);
    }

    pub const fn take_select(&mut self) -> Option<Vec<OsString>> {
        self.selects.take()
    }

    pub fn select_when_listed(
        &mut self,
        folder: PathBuf,
        names: Vec<OsString>,
        cx: &mut Context<Self>,
    ) {
        self.selects_later = Some((folder, names));
        cx.emit(EntriesAsked);
    }

    pub const fn take_select_later(&mut self) -> Option<(PathBuf, Vec<OsString>)> {
        self.selects_later.take()
    }
}
