use std::path::PathBuf;

use gpui::Context;
use nova_files_api::{Entry, EntryId, Listing};

use crate::location::Location;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Shown {
    pub location: Option<Location>,
    pub listing: Option<Listing>,
    pub selected: Vec<EntryId>,
    pub picker: bool,
}

#[derive(Debug, Default)]
pub struct Entries {
    shown: Shown,
}

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

    pub fn selected_count(&self) -> usize {
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
}
