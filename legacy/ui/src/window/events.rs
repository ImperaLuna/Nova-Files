use std::collections::HashMap;
use std::mem;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{Context, UniformListScrollHandle};
use nova_files_api::{Event, Listing, ListingId, Unreadable};
use nova_files_places_ui::Known;
use nova_files_settings_api::{Settings, Thumbnails};
use nova_files_shell::Published;

use super::rows::release;
use super::{CoreStatus, MainWindow, WindowEvent};
use crate::island;
use crate::jump::TypeJump;
use crate::location::Location;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Place {
    Kept,
    Top,
}

impl MainWindow {
    fn listing_arrived(&mut self, listing: Listing, cx: &mut Context<Self>) {
        self.asked = None;
        self.search_left(cx);
        let arrived = match self.requested {
            Some(Location::Gathered(gathered)) => Location::Gathered(gathered),
            _ => Location::from(listing.path.as_path()),
        };
        let same_folder =
            !self.showing_found() && self.place_on_screen_now().as_ref() == Some(&arrived);
        if !same_folder {
            self.menu = None;
            let fell_back = mem::take(&mut self.falling_back);
            if let (Location::Folder(folder), false) = (&arrived, fell_back) {
                self.used(folder);
            }
        }
        self.gathered.shown = match &arrived {
            Location::Gathered(place) => Some(*place),
            Location::Start | Location::Folder(_) | Location::Properties(_) | Location::Page(_) => {
                None
            }
        };
        self.history.arrived(&arrived);
        let reordered = same_folder && self.shown_order != self.asked_order;
        self.shown_order = self.asked_order;
        let shown = self.shown_selection(&listing);
        let wanted = self.wanted_selection(&listing).or(shown);
        let listing_path = listing.path.clone();
        let made = wanted.is_some();
        self.selection = wanted.or_else(|| self.selection_for(&listing, same_folder));
        if arrived.folder().is_some() {
            self.tree_follows(&listing, !same_folder);
        }
        self.ask_disk_space();
        self.search.reveal = None;
        let place = if same_folder { Place::Kept } else { Place::Top };
        self.listing_replaced(listing, place, cx);
        self.pick_folder_opened(reordered, cx);
        self.typed_path_arrived(same_folder);
        if (!same_folder || reordered || made)
            && let Some(cursor) = self.cursor()
        {
            self.scroll_to_entry(cursor);
        }
        self.tell_parts_listed(reordered, cx);
        cx.emit(match arrived {
            Location::Gathered(gathered) => WindowEvent::GatheredShown(gathered),
            Location::Start | Location::Folder(_) | Location::Properties(_) | Location::Page(_) => {
                WindowEvent::Opened(listing_path)
            }
        });
        cx.notify();
    }

    pub(super) fn place_on_screen_now(&self) -> Option<Location> {
        self.listing.as_ref()?;
        self.gathered_shown()
            .map(Location::Gathered)
            .or_else(|| self.folder().map(Location::from))
    }

    pub(super) fn listing_replaced(
        &mut self,
        listing: Listing,
        place: Place,
        cx: &mut Context<Self>,
    ) {
        self.counted = island::count_of(&listing.entries).into();
        let before = self.listing.replace(listing);
        self.core_moved = false;
        self.details = HashMap::new();
        release(self.thumbnails.take_all(), None, cx);
        self.rows_wanted = 0..0;
        if place == Place::Top {
            self.scroll = UniformListScrollHandle::new();
            self.jump = TypeJump::default();
            self.band = None;
            self.island.confirm = None;
        }
        self.preview_listing_arrived(before.as_ref(), cx);
    }

    pub(super) fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
        self.parts.watch(&event, cx);
        match event {
            Event::Pong => self.set_status(CoreStatus::Connected, cx),
            Event::FolderListed(listing) if self.asked == Some(listing.id) => {
                self.listing_arrived(listing, cx);
            }
            Event::FolderListed(listing) if self.search.asked == Some(listing.id) => {
                self.found_arrived(listing, cx);
            }
            Event::FindFailed { id, .. } if self.search.asked == Some(id) => {
                self.find_failed(id, cx);
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
            Event::Thumbnail {
                id,
                entry,
                size,
                png,
            } => self.thumbnail_arrived((id, entry, size), png, cx),
            Event::FolderListed(stale) => self.dropped_listing(stale.id, cx),
            Event::Previewed { id, preview } => self.preview_arrived(id, Some(preview), cx),
            Event::PreviewUnreadable { id, .. } => self.preview_arrived(id, None, cx),
            Event::OpenWith(answer) => self.open_with_answered(answer, cx),
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
            | Event::Found { .. }
            | Event::SearchFailed { .. }
            | Event::FindFailed { .. }
            | Event::FileOpened { .. }
            | Event::WindowState(_)
            | Event::JobRunning { .. }
            | Event::JobInterrupted { .. } => {}
            Event::FileNotOpened { path, reason } => self.file_not_opened(path, reason, cx),
            Event::FolderViews(views) => self.folder_views_arrived(&views, cx),
        }
    }

    pub(super) fn places_changed(&mut self, cx: &mut Context<Self>) {
        if let Some(known) = Published::<Known>::get(cx) {
            self.places.clone_from(&known.places);
            self.pinned = Arc::clone(&known.pinned);
        }
        self.sidebar_changed(cx);
    }

    fn sidebar_changed(&mut self, cx: &mut Context<Self>) {
        if self.is_on_start() {
            self.ask_for_recent();
        }
        cx.emit(WindowEvent::Places);
        cx.notify();
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
                    self.read_again(cx);
                }
            }
        }
        self.ask_for_recent();
        self.pick_folder_refused(id, &path);
        self.parts.refused(id, &path, cx);
        self.typed_path_refused(id, cx);
        cx.emit(WindowEvent::Refused(path));
        cx.notify();
    }

    pub(super) fn settings_arrived(&mut self, settings: Arc<Settings>, cx: &mut Context<Self>) {
        let thumbnails_changed = settings.thumbnails != self.settings.thumbnails;
        let hidden_files = settings.hidden_files;
        self.settings = Arc::unwrap_or_clone(settings);
        self.hidden_arrived(hidden_files);
        if thumbnails_changed {
            if self.settings.thumbnails == Thumbnails::Hidden {
                release(self.thumbnails.take_all(), None, cx);
            }
            self.forget_rows();
        }
        cx.emit(WindowEvent::Settings);
        cx.notify();
    }

    fn dropped_listing(&mut self, id: ListingId, cx: &Context<Self>) {
        let Some(listing) = &self.listing else { return };
        if id <= listing.id {
            return;
        }
        self.core_moved = true;
        if self.asked.is_none() && self.search.asked.is_none() {
            self.read_again(cx);
        }
    }

    fn read_again(&mut self, cx: &Context<Self>) {
        if self.searching() && self.showing_found() {
            self.find(cx);
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
