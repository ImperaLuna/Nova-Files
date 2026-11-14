//! The core's events as the window takes them: a listing arriving or
//! refused, details, thumbnails, places, settings and the theme. Listings
//! the window no longer wants are dropped here too, and the folder on
//! screen read again when the core moved on from it (see `window`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{Context, ScrollStrategy, UniformListScrollHandle};
use nova_files_api::{Event, Listing, ListingId, Settings, Theme, Thumbnails};

use super::rows::release;
use super::{CoreStatus, MainWindow, WindowEvent};
use crate::jump::TypeJump;
use crate::location::Location;
use crate::theme::Colors;

impl MainWindow {
    /// Shows `listing`, the folder the window last asked for.
    fn listing_arrived(&mut self, listing: Listing, cx: &mut Context<Self>) {
        self.asked = None;
        let path = listing.path.clone();
        // Read again in place: keep the scroll position.
        let same_folder = self.folder() == Some(path.as_path());
        if !same_folder && !std::mem::take(&mut self.falling_back) {
            self.used(&path);
        }
        self.history.arrived(&Location::from(path.as_path()));
        // In a new order the cursor's entry has moved: bring it into view.
        let reordered = same_folder && self.shown_order != self.asked_order;
        self.shown_order = self.asked_order;
        self.selection = self.selection_for(&listing);
        self.listing = Some(listing);
        self.core_moved = false;
        self.details = HashMap::new();
        release(self.thumbnails.take_all(), None, cx);
        self.rows_wanted = 0..0;
        if !same_folder {
            self.scroll = UniformListScrollHandle::new();
            self.jump = TypeJump::default();
            self.band = None;
        }
        self.pick_folder_opened(reordered, cx);
        if (!same_folder || reordered)
            && let Some(cursor) = self.cursor()
        {
            self.scroll.scroll_to_item(cursor, ScrollStrategy::Nearest);
        }
        cx.emit(WindowEvent::Opened(path));
        cx.notify();
    }

    pub(super) fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Pong => self.set_status(CoreStatus::Connected, cx),
            Event::FolderListed(listing) if self.asked == Some(listing.id) => {
                self.listing_arrived(listing, cx);
            }
            Event::FolderUnreadable { id, path, .. } if self.asked == Some(id) => {
                self.listing_refused(id, path, cx);
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
            Event::FolderListed(stale) => self.dropped_listing(stale.id),
            Event::Settings(settings) => self.settings_arrived(settings, cx),
            Event::Theme(theme) => Self::theme_arrived(&theme, cx),
            // The window never searches, opens files, starts jobs or asks
            // for drives yet, and only a picker's opening asks for the
            // window state (see `window_state`).
            Event::FolderUnreadable { .. }
            | Event::Drives(_)
            | Event::DiskSpace(_)
            | Event::RecentFolders { .. }
            | Event::DriveDone { .. }
            | Event::Found { .. }
            | Event::SearchFailed { .. }
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
            | Event::Bookmarks(_)
            | Event::OpenWith(_)
            | Event::SizeMeasured { .. }
            | Event::SpaceMeasured { .. }
            | Event::SpaceUnreadable { .. }
            | Event::Trash(_)
            | Event::Undoable { .. }
            | Event::UndoGone(_) => {}
            Event::Places(places) => {
                self.places = places;
                cx.emit(WindowEvent::Places);
                cx.notify();
            }
        }
    }

    /// Listing `id` of `path`, the one asked for, can't be read: the
    /// window stays where it is.
    fn listing_refused(&mut self, id: ListingId, path: PathBuf, cx: &mut Context<Self>) {
        self.asked = None;
        self.history.refused(&Location::from(path.as_path()));
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
            Some(listing) if self.core_moved && listing.path != path => {
                let shown = listing.path.clone();
                self.request(&shown);
            }
            Some(_) => {}
        }
        self.pick_folder_refused(id, &path);
        cx.emit(WindowEvent::Refused(path));
        // Back or Forward may have nowhere left to go.
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
    /// than the folder on screen moved the core's row queues to it. A
    /// refusal still to come reads the folder on screen again (see
    /// `on_event`); with nothing asked for, nothing would, so it is read
    /// again now.
    fn dropped_listing(&mut self, id: ListingId) {
        let Some(listing) = &self.listing else { return };
        if id <= listing.id {
            return;
        }
        self.core_moved = true;
        if self.asked.is_none() {
            let shown = listing.path.clone();
            self.request(&shown);
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
