use std::collections::HashMap;
use std::mem;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{Context, UniformListScrollHandle};
use nova_files_api::{Event, FolderSizes, HiddenFiles, Listing, ListingId, Unreadable};
use nova_files_shell::{KnownFolders, Published, ThumbnailsShown};

use super::{CoreStatus, MainWindow, WindowEvent};
use crate::island;
use crate::jump::TypeJump;
use nova_files_shell::Location;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Place {
    Kept,
    Top,
}

impl MainWindow {
    fn listing_arrived(&mut self, listing: Listing, cx: &mut Context<Self>) {
        self.asked = None;
        self.search_left(cx);
        let arrived = match &self.requested {
            Some(Location::Page(page)) => Location::Page(page.clone()),
            _ => Location::from(listing.path.as_path()),
        };
        let same_folder =
            !self.showing_found() && self.place_on_screen_now().as_ref() == Some(&arrived);
        if !same_folder {
            self.menu = None;
            let fell_back = mem::take(&mut self.falling_back);
            if let (Location::Folder(folder), false) = (&arrived, fell_back) {
                self.used(folder, cx);
            }
        }
        self.listed_page = match &arrived {
            Location::Page(page) if !page.is_start() => Some(page.clone()),
            Location::Page(_) | Location::Folder(_) => None,
        };
        self.history.arrived(&arrived);
        let reordered = same_folder && self.shown_order != self.asked_order;
        self.shown_order = self.asked_order;
        let shown = self.shown_selection(&listing);
        let first = self.first_match(&listing);
        let wanted = shown.or(first);
        let listing_path = listing.path.clone();
        let made = wanted.is_some();
        self.selection = wanted.or_else(|| self.selection_for(&listing, same_folder));
        self.search.reveal = None;
        let place = if same_folder { Place::Kept } else { Place::Top };
        self.listing_replaced(listing, place);
        if !reordered && self.picker() {
            self.selection = None;
        }
        self.path_bar
            .update(cx, |bar, cx| bar.arrived(same_folder, cx));
        if (!same_folder || reordered || made)
            && let Some(cursor) = self.cursor()
        {
            self.scroll_to_entry(cursor, cx);
        }
        self.tell_parts_listed(reordered, cx);
        cx.emit(WindowEvent::Opened(listing_path));
        cx.notify();
    }

    pub(super) fn place_on_screen_now(&self) -> Option<Location> {
        self.listing.as_ref()?;
        self.listed_page
            .clone()
            .map(Location::Page)
            .or_else(|| self.folder().map(Location::from))
    }

    pub(super) fn listing_replaced(&mut self, listing: Listing, place: Place) {
        self.counted = island::count_of(&listing.entries).into();
        self.listing = Some(listing);
        self.core_moved = false;
        self.details = HashMap::new();
        self.shares.forget(place == Place::Top);
        self.rows_wanted = 0..0;
        if place == Place::Top {
            self.scroll = UniformListScrollHandle::new();
            self.jump = TypeJump::default();
            self.band = None;
            self.island.confirm = None;
        }
    }

    pub(super) fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
        self.parts.watch(&event, cx);
        match event {
            Event::Pong => self.set_status(CoreStatus::Connected, cx),
            Event::FolderListed(listing) if self.asked == Some(listing.id) => {
                self.listing_arrived(listing, cx);
            }
            Event::FolderListed(listing) if self.sorted_again(&listing) => {
                self.listing_arrived(listing, cx);
            }
            Event::FolderListed(listing) if self.search.asked == Some(listing.id) => {
                self.found_arrived(listing, cx);
            }
            Event::FolderUnreadable { id, path, reason } if self.asked == Some(id) => {
                self.folder_refused(id, path, reason, cx);
            }
            Event::DetailsRead {
                id,
                first,
                details,
                icons,
            } if self
                .listing
                .as_ref()
                .is_some_and(|listing| listing.id == id) =>
            {
                self.details_arrived(first, &details, &icons, cx);
            }
            Event::FoldersMeasured { id, sizes }
                if self
                    .listing
                    .as_ref()
                    .is_some_and(|listing| listing.id == id) =>
            {
                self.folders_measured(&sizes, cx);
            }
            Event::FolderListed(stale) => self.dropped_listing(stale.id),
            Event::WindowState(bytes) => {
                self.host
                    .kept
                    .update(cx, |kept, cx| kept.arrived(&bytes, cx));
            }
            event @ (Event::JobProgress { .. }
            | Event::ConflictAnswered { .. }
            | Event::JobPaused(_)
            | Event::JobResumed(_)) => self.job_moved(&event, cx),
            event @ (Event::JobStarted { .. }
            | Event::JobFinished { .. }
            | Event::JobConflict { .. }
            | Event::Undoable { .. }
            | Event::UndoGone(_)) => self.job_event(event, cx),
            Event::FolderUnreadable { .. }
            | Event::DetailsRead { .. }
            | Event::FoldersMeasured { .. }
            | Event::JobRunning { .. }
            | Event::JobInterrupted { .. } => {}
        }
    }

    pub(super) fn listing_failed(&mut self, id: ListingId, cx: &mut Context<Self>) {
        if self.asked != Some(id) {
            return;
        }
        let path = self
            .requested
            .as_ref()
            .and_then(Location::folder)
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let empty = Listing {
            id,
            path,
            entries: Arc::from([]),
            folders: None,
        };
        self.listing_arrived(empty, cx);
    }

    fn missing_named(&self, id: ListingId, path: &Path) -> Option<Listing> {
        let named = self.host.rules.named(path)?;
        named.empty_when_missing.then(|| Listing {
            id,
            path: path.to_path_buf(),
            entries: Arc::from([]),
            folders: None,
        })
    }

    fn folder_refused(
        &mut self,
        id: ListingId,
        path: PathBuf,
        reason: Unreadable,
        cx: &mut Context<Self>,
    ) {
        if reason == Unreadable::Missing
            && let Some(empty) = self.missing_named(id, &path)
        {
            self.listing_arrived(empty, cx);
            return;
        }
        if self.tab_folder_refused(id, cx) {
            cx.emit(WindowEvent::Refused(path));
            return;
        }
        self.asked = None;
        let refused = self
            .requested
            .clone()
            .unwrap_or_else(|| Location::from(path.as_path()));
        self.history.refused(&refused);
        match &self.listing {
            None => {
                if self.history.here().is_none()
                    && let Some(fallback) = self.fallbacks.pop()
                {
                    self.falling_back = self.request(&fallback);
                }
            }
            Some(_) => {
                if mem::take(&mut self.core_moved) && self.place_on_screen_now() != Some(refused) {
                    self.read_again();
                }
            }
        }
        self.parts.refused(id, &path, cx);
        self.path_bar.update(cx, |bar, cx| bar.refused(id, cx));
        cx.emit(WindowEvent::Refused(path));
        cx.notify();
    }

    pub(super) fn hidden_published(&mut self, cx: &mut Context<Self>) {
        if let Some(hidden) = Published::<HiddenFiles>::get(cx).copied() {
            self.hidden_arrived(hidden);
            cx.emit(WindowEvent::Settings);
            cx.notify();
        }
    }

    pub(super) fn folder_sizes_published(&mut self, cx: &mut Context<Self>) {
        if let Some(sizes) = Published::<FolderSizes>::get(cx).copied() {
            self.folder_sizes_arrived(sizes);
            cx.emit(WindowEvent::Settings);
            cx.notify();
        }
    }

    pub(super) fn known_published(&mut self, cx: &mut Context<Self>) {
        if let Some(known) = Published::<KnownFolders>::get(cx) {
            self.known = known.clone();
            cx.notify();
        }
    }

    pub(super) fn thumbnails_published(&mut self, cx: &mut Context<Self>) {
        let Some(shown) = Published::<ThumbnailsShown>::get(cx).copied() else {
            return;
        };
        if shown == self.thumbnails_shown {
            return;
        }
        self.thumbnails_shown = shown;
        self.forget_rows(cx);
        cx.emit(WindowEvent::Settings);
        cx.notify();
    }

    fn dropped_listing(&mut self, id: ListingId) {
        let Some(listing) = &self.listing else { return };
        if id <= listing.id {
            return;
        }
        self.core_moved = true;
        if self.asked.is_none() && self.search.asked.is_none() {
            self.read_again();
        }
    }

    fn read_again(&mut self) {
        if self.searching() && self.showing_found() {
            self.find();
        } else if let Some(place) = self.place_on_screen_now() {
            self.request_place(&place);
        }
    }

    pub(super) fn set_status(&mut self, status: CoreStatus, cx: &mut Context<Self>) {
        if self.status != status {
            self.status = status;
            if matches!(status, CoreStatus::Gone) {
                self.jobs = super::jobs::Jobs::default();
            }
            cx.emit(WindowEvent::Status(status));
            cx.notify();
        }
    }
}
