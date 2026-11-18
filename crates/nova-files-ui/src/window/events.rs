//! The core's events as the window takes them: a listing arriving or
//! refused, a search's files, details, thumbnails, previews, places,
//! pinned folders, drives, settings, the theme, and jobs (handed to
//! `jobs`). Listings
//! the window no longer wants are dropped here too, and the folder on
//! screen read again when the core moved on from it (see `window`).

use std::collections::HashMap;
use std::mem;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{Context, UniformListScrollHandle};
use nova_files_api::{
    Bookmark, Event, Listing, ListingId, Settings, Theme, Thumbnails, Unreadable,
};

use super::rows::release;
use super::{CoreStatus, MainWindow, WindowEvent};
use crate::island;
use crate::jump::TypeJump;
use crate::location::Location;
use crate::theme::Colors;

/// Where a list put on screen starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Place {
    /// Where the list before it was: the same folder, read again.
    Kept,
    /// At its top, with nothing typed or dragged carried over.
    Top,
}

impl MainWindow {
    /// Shows `listing`, the place the window last asked for: a folder, or
    /// Games or Projects.
    fn listing_arrived(&mut self, listing: Listing, cx: &mut Context<Self>) {
        self.asked = None;
        self.search_left(cx);
        let arrived = match self.requested {
            Some(Location::Gathered(gathered)) => Location::Gathered(gathered),
            _ => Location::from(listing.path.as_path()),
        };
        // Read again in place: keep the scroll position. Files a search
        // found are never the same place.
        let same_folder =
            !self.showing_found() && self.place_on_screen_now().as_ref() == Some(&arrived);
        if !same_folder {
            // A menu belongs to the place it opened in.
            self.menu = None;
            let fell_back = mem::take(&mut self.falling_back);
            if let (Location::Folder(folder), false) = (&arrived, fell_back) {
                self.used(folder);
            }
        }
        self.gathered.shown = match &arrived {
            Location::Gathered(place) => Some(*place),
            Location::Start | Location::Folder(_) | Location::Properties(_) => None,
        };
        self.history.arrived(&arrived);
        // In a new order the cursor's entry has moved: bring it into view.
        let reordered = same_folder && self.shown_order != self.asked_order;
        self.shown_order = self.asked_order;
        // What a rename or a new folder made, or what an app asked to
        // show, is brought into view.
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
        // Jobs that change the trash ask again (see `trash`).
        if !same_folder && self.trash_open() {
            self.list_trash();
        }
        self.typed_path_arrived(same_folder);
        if (!same_folder || reordered || made)
            && let Some(cursor) = self.cursor()
        {
            self.scroll_to_entry(cursor);
        }
        cx.emit(match arrived {
            Location::Gathered(gathered) => WindowEvent::GatheredShown(gathered),
            Location::Start | Location::Folder(_) | Location::Properties(_) => {
                WindowEvent::Opened(listing_path)
            }
        });
        cx.notify();
    }

    /// The place whose listing is on screen: the folder, or Games or
    /// Projects; a search under way counts as its folder.
    pub(super) fn place_on_screen_now(&self) -> Option<Location> {
        self.listing.as_ref()?;
        self.gathered_shown()
            .map(Location::Gathered)
            .or_else(|| self.folder().map(Location::from))
    }

    /// Puts `listing` on screen in place of the one before, whose details
    /// and thumbnails go with it, the list at `place`.
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
            // A question was about the folder left.
            self.island.confirm = None;
        }
        self.preview_listing_arrived(before.as_ref(), cx);
    }

    pub(super) fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
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
            Event::Properties { id, properties } => {
                self.save_probed(id, false, cx);
                self.properties_arrived(id, Some(properties), cx);
            }
            Event::PropertiesUnreadable { id, reason, .. } => {
                self.save_probed(id, reason == Unreadable::Missing, cx);
                self.properties_arrived(id, None, cx);
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
            // An answer for a folder the window has since moved away from.
            Event::FolderListed(stale) => self.dropped_listing(stale.id, cx),
            Event::Previewed { id, preview } => self.preview_arrived(id, Some(preview), cx),
            Event::PreviewUnreadable { id, .. } => self.preview_arrived(id, None, cx),
            Event::Settings(settings) => self.settings_arrived(settings, cx),
            Event::Drives(_) | Event::DriveDone { .. } | Event::DiskSpace(_) => {
                self.drive_event(event, cx);
            }
            Event::RecentFolders { id, folders } => self.recent_arrived(id, folders, cx),
            Event::SizeMeasured { id, size, state } => self.size_measured(id, size, state, cx),
            Event::Theme(theme) => Self::theme_arrived(&theme, cx),
            Event::DropWould { id, kind } => self.drop_would_arrived(id, kind, cx),
            event @ (Event::JobProgress { .. }
            | Event::ConflictAnswered { .. }
            | Event::JobPaused(_)
            | Event::JobResumed(_)) => self.job_moved(&event, cx),
            event @ (Event::JobStarted { .. }
            | Event::JobFinished { .. }
            | Event::JobConflict { .. }
            | Event::Undoable { .. }
            | Event::UndoGone(_)) => self.job_event(event, cx),
            // The window doesn't wait on a file opening and never watches
            // jobs it didn't start; only a picker's opening asks for the
            // window state (see `window_state`). Details that come for a
            // folder the window has since left are dropped.
            Event::FolderUnreadable { .. }
            | Event::DetailsRead { .. }
            | Event::Found { .. }
            | Event::SearchFailed { .. }
            | Event::FindFailed { .. }
            | Event::FileOpened { .. }
            | Event::WindowState(_)
            | Event::JobRunning { .. }
            | Event::JobInterrupted { .. }
            | Event::OpenWith(_)
            | Event::SpaceMeasured { .. }
            | Event::SpaceUnreadable { .. } => {}
            Event::Gathered(gathered) => self.gathered_arrived(gathered, cx),
            Event::GatheredChanged(place) => self.gathered_changed(place),
            Event::FileNotOpened { path, reason } => self.file_not_opened(path, reason, cx),
            Event::Trash(items) => self.trash_listed(items, cx),
            Event::Places(places) => self.places_arrived(places, cx),
            Event::FolderViews(views) => self.folder_views_arrived(&views, cx),
            Event::Bookmarks(pinned) => self.pinned_arrived(pinned, cx),
        }
    }

    fn places_arrived(&mut self, places: Vec<nova_files_api::Place>, cx: &mut Context<Self>) {
        self.places = places;
        self.sidebar_changed(cx);
    }

    fn pinned_arrived(&mut self, pinned: Arc<[Bookmark]>, cx: &mut Context<Self>) {
        self.pinned = pinned;
        self.sidebar_changed(cx);
    }

    /// Start leaves out the sidebar's folders, which have changed.
    fn sidebar_changed(&mut self, cx: &mut Context<Self>) {
        if self.is_on_start() {
            self.ask_for_recent();
        }
        cx.emit(WindowEvent::Places);
        cx.notify();
    }

    /// Listing `id` of `path`, the one asked for, can't be read: the
    /// window stays where it is. A trash never made yet shows empty (see
    /// `trash`).
    fn folder_refused(
        &mut self,
        id: ListingId,
        path: PathBuf,
        reason: Unreadable,
        cx: &mut Context<Self>,
    ) {
        if reason == Unreadable::Missing
            && let Some(empty) = self.missing_trash(id, &path)
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
            // A window that has been nowhere yet tries the next
            // fallback; one on Start stays there.
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
        // A recent folder's card that can't be opened goes from Start.
        self.ask_for_recent();
        self.pick_folder_refused(id, &path);
        self.typed_path_refused(id, cx);
        cx.emit(WindowEvent::Refused(path));
        // Back or Forward may have nowhere left to go.
        cx.notify();
    }

    /// Takes the settings the core sent, which it sends again whenever
    /// they change. Hiding thumbnails lets go of the ones held; showing
    /// them again asks for the rows on screen anew, so theirs arrive.
    fn settings_arrived(&mut self, settings: Arc<Settings>, cx: &mut Context<Self>) {
        let thumbnails_changed = settings.thumbnails != self.settings.thumbnails;
        let hidden_files = settings.hidden_files;
        self.settings = Arc::unwrap_or_clone(settings);
        self.hidden_arrived(hidden_files);
        if thumbnails_changed {
            if self.settings.thumbnails == Thumbnails::Hidden {
                release(self.thumbnails.take_all(), None, cx);
            }
            // Hidden and shown again before a frame, the core would still
            // count the dropped thumbnails as sent.
            self.forget_rows();
        }
        cx.emit(WindowEvent::Settings);
        cx.notify();
    }

    /// Draws every window with `theme` from now on. Each window hears it,
    /// so the first to see a change redraws them all and the rest find
    /// nothing new.
    fn theme_arrived(theme: &Theme, cx: &mut Context<Self>) {
        let colors = Colors::new(theme);
        if Colors::of(cx) != colors {
            cx.set_global(colors);
            cx.refresh_windows();
            cx.emit(WindowEvent::Theme);
        }
    }

    /// The window dropped listing `id`, which it no longer wanted. One newer
    /// than the list on screen moved the core's row queues to it. A
    /// refusal still to come reads the folder on screen again (see
    /// `on_event`); with nothing asked for, nothing would, so what is on
    /// screen is asked for again now.
    fn dropped_listing(&mut self, id: ListingId, cx: &Context<Self>) {
        let Some(listing) = &self.listing else { return };
        if id <= listing.id {
            return;
        }
        self.core_moved = true;
        // A find still on its way moves the core's row queues on too.
        if self.asked.is_none() && self.search.asked.is_none() {
            self.read_again(cx);
        }
    }

    /// Asks again for what is on screen, so the core serves its rows: the
    /// search's files while it shows them, otherwise the folder or
    /// gathered place.
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
                // No job of this window will say more.
                self.jobs = super::jobs::Jobs::default();
            }
            cx.emit(WindowEvent::Status(status));
            cx.notify();
        }
    }
}
