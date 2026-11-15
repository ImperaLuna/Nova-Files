//! The core's events as the window takes them: a listing arriving or
//! refused, a search's files, details, thumbnails, places, pinned folders,
//! settings and the theme. Listings
//! the window no longer wants are dropped here too, and the folder on
//! screen read again when the core moved on from it (see `window`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{Context, ScrollStrategy, UniformListScrollHandle};
use nova_files_api::{Event, Listing, ListingId, Settings, Theme, Thumbnails};

use super::rows::release;
use super::{CoreStatus, MainWindow, WindowEvent};
use crate::jump::TypeJump;
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
    /// Shows `listing`, the folder the window last asked for.
    fn listing_arrived(&mut self, listing: Listing, cx: &mut Context<Self>) {
        self.asked = None;
        self.search_left(cx);
        let path = listing.path.clone();
        self.history.arrived(&path);
        // Read again in place: keep the scroll position. Files a search
        // found are never the same folder.
        let same_folder = !self.showing_found() && self.folder() == Some(path.as_path());
        // In a new order the cursor's entry has moved: bring it into view.
        let reordered = same_folder && self.shown_order != self.asked_order;
        self.shown_order = self.asked_order;
        self.selection = self.selection_for(&listing);
        self.search.reveal = None;
        let place = if same_folder { Place::Kept } else { Place::Top };
        self.listing_replaced(listing, place, cx);
        self.pick_folder_opened(reordered, cx);
        self.typed_path_arrived(same_folder);
        if (!same_folder || reordered)
            && let Some(cursor) = self.cursor()
        {
            self.scroll.scroll_to_item(cursor, ScrollStrategy::Nearest);
        }
        cx.emit(WindowEvent::Opened(path));
        cx.notify();
    }

    /// Puts `listing` on screen in place of the one before, whose details
    /// and thumbnails go with it, the list at `place`.
    pub(super) fn listing_replaced(
        &mut self,
        listing: Listing,
        place: Place,
        cx: &mut Context<Self>,
    ) {
        self.listing = Some(listing);
        self.core_moved = false;
        self.details = HashMap::new();
        release(self.thumbnails.take_all(), None, cx);
        self.rows_wanted = 0..0;
        if place == Place::Top {
            self.scroll = UniformListScrollHandle::new();
            self.jump = TypeJump::default();
            self.band = None;
        }
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
            Event::FolderUnreadable { id, path, .. } if self.asked == Some(id) => {
                self.folder_refused(id, path, cx);
            }
            Event::DetailsRead { id, first, details } => {
                if let Some(listing) = &self.listing
                    && listing.id == id
                    && let Ok(first) = usize::try_from(first.0)
                {
                    let rows = first..listing.entries.len();
                    self.details.extend(rows.zip(details.iter().copied()));
                    cx.notify();
                }
            }
            // Checked against the listing once shrunk, when it counts.
            Event::Thumbnail { id, entry, png } => {
                if let Ok(index) = usize::try_from(entry.0)
                    && self.rows_wanted.contains(&index)
                    && self.settings.thumbnails == Thumbnails::Shown
                {
                    self.shrink_thumbnail(id, index, png, cx);
                }
            }
            // An answer for a folder the window has since moved away from.
            Event::FolderListed(stale) => self.dropped_listing(stale.id, cx),
            Event::Settings(settings) => self.settings_arrived(settings, cx),
            Event::Theme(theme) => Self::theme_arrived(&theme, cx),
            // The window never searches, opens files, starts jobs or asks
            // for drives yet, and only a picker's opening asks for the
            // window state (see `window_state`).
            Event::FolderUnreadable { .. }
            | Event::Drives(_)
            | Event::DiskSpace(_)
            | Event::DriveDone { .. }
            | Event::Found { .. }
            | Event::SearchFailed { .. }
            | Event::FindFailed { .. }
            | Event::Properties { .. }
            | Event::PropertiesUnreadable { .. }
            | Event::FileOpened
            | Event::FileNotOpened
            | Event::WindowState(_)
            | Event::JobStarted { .. }
            | Event::JobRunning { .. }
            | Event::JobConflict { .. }
            | Event::ConflictAnswered { .. }
            | Event::JobProgress { .. }
            | Event::JobPaused(_)
            | Event::JobResumed(_)
            | Event::JobInterrupted { .. }
            | Event::JobFinished { .. }
            | Event::OpenWith(_)
            | Event::SizeMeasured { .. }
            | Event::SpaceMeasured { .. }
            | Event::SpaceUnreadable { .. }
            | Event::Trash(_)
            | Event::Undoable { .. }
            | Event::UndoGone(_) => {}
            Event::Places(places) => {
                self.places = places;
                Self::places_changed(cx);
            }
            Event::Bookmarks(pinned) => {
                self.pinned = pinned;
                Self::places_changed(cx);
            }
        }
    }

    /// The folder asked for last, `path`, couldn't be read: the window
    /// stays put, or falls back when it has nothing on screen yet.
    fn folder_refused(&mut self, id: ListingId, path: PathBuf, cx: &mut Context<Self>) {
        self.asked = None;
        self.history.refused(&path);
        match &self.listing {
            None => {
                if let Some(fallback) = self.fallbacks.pop() {
                    self.request(&fallback);
                }
            }
            Some(listing) if self.core_moved && listing.path != path => {
                let shown = listing.path.clone();
                self.read_again(&shown, cx);
            }
            Some(_) => {}
        }
        self.pick_folder_refused(id, &path);
        self.typed_path_refused(id, cx);
        cx.emit(WindowEvent::Refused(path));
        // Back or Forward may have nowhere left to go.
        cx.notify();
    }

    fn places_changed(cx: &mut Context<Self>) {
        cx.emit(WindowEvent::Places);
        cx.notify();
    }

    /// Takes the settings the core sent, which it sends again whenever
    /// they change. Hiding thumbnails lets go of the ones held; showing
    /// them again asks for the rows on screen anew, so theirs arrive.
    fn settings_arrived(&mut self, settings: Arc<Settings>, cx: &mut Context<Self>) {
        let thumbnails_changed = settings.thumbnails != self.settings.thumbnails;
        self.settings = Arc::unwrap_or_clone(settings);
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
            let shown = listing.path.clone();
            self.read_again(&shown, cx);
        }
    }

    /// Asks again for what is on screen, so the core serves its rows: the
    /// search's files while it shows them, otherwise `shown`, the folder.
    fn read_again(&mut self, shown: &Path, cx: &Context<Self>) {
        if self.searching() && self.showing_found() {
            self.find(cx);
        } else {
            self.request(shown);
        }
    }

    pub(super) fn set_status(&mut self, status: CoreStatus, cx: &mut Context<Self>) {
        if self.status != status {
            self.status = status;
            cx.emit(WindowEvent::Status(status));
            cx.notify();
        }
    }
}
