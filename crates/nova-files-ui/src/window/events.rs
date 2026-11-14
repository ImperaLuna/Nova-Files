//! The core's events as the window takes them: a listing arriving or
//! refused, details, thumbnails, places, settings, the theme, and jobs
//! (handed to `jobs`). Listings
//! the window no longer wants are dropped here too, and the folder on
//! screen read again when the core moved on from it (see `window`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{Context, ScrollStrategy, UniformListScrollHandle};
use nova_files_api::{Event, Listing, ListingId, Settings, Theme, Thumbnails, Unreadable};

use super::rows::release;
use super::{CoreStatus, MainWindow, WindowEvent};
use crate::jump::TypeJump;
use crate::theme::Colors;

impl MainWindow {
    /// Shows `listing`, the folder the window last asked for.
    fn listing_arrived(&mut self, listing: Listing, cx: &mut Context<Self>) {
        self.asked = None;
        let path = listing.path.clone();
        self.history.arrived(&path);
        // Read again in place: keep the scroll position.
        let same_folder = self.folder() == Some(path.as_path());
        // In a new order the cursor's entry has moved: bring it into view.
        let reordered = same_folder && self.shown_order != self.asked_order;
        self.shown_order = self.asked_order;
        // What a rename or a new folder made is brought into view.
        let wanted = self.wanted_selection(&listing);
        let made = wanted.is_some();
        self.selection = wanted.or_else(|| self.selection_for(&listing));
        self.listing = Some(listing);
        self.core_moved = false;
        self.details = HashMap::new();
        release(self.thumbnails.take_all(), None, cx);
        self.rows_wanted = 0..0;
        if !same_folder {
            self.scroll = UniformListScrollHandle::new();
            self.jump = TypeJump::default();
            self.band = None;
            // A question was about the folder left.
            self.island.confirm = None;
        }
        self.pick_folder_opened(reordered, cx);
        // Jobs that change the trash ask again (see `trash`).
        if !same_folder && self.trash_open() {
            self.list_trash();
        }
        if (!same_folder || reordered || made)
            && let Some(cursor) = self.cursor()
        {
            self.scroll.scroll_to_item(cursor, ScrollStrategy::Nearest);
        }
        cx.emit(WindowEvent::Opened(path));
        cx.notify();
    }

    /// The folder asked for as `id` couldn't be read. A trash never made
    /// yet shows empty (see `trash`).
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
                self.request(&shown);
            }
            Some(_) => {}
        }
        self.pick_folder_refused(id, &path);
        cx.emit(WindowEvent::Refused(path));
        // Back or Forward may have nowhere left to go.
        cx.notify();
    }

    pub(super) fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Pong => self.set_status(CoreStatus::Connected, cx),
            Event::FolderListed(listing) if self.asked == Some(listing.id) => {
                self.listing_arrived(listing, cx);
            }
            Event::FolderUnreadable { id, path, reason } if self.asked == Some(id) => {
                self.folder_refused(id, path, reason, cx);
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
            event @ (Event::JobStarted { .. }
            | Event::JobFinished { .. }
            | Event::JobProgress { .. }
            | Event::JobConflict { .. }
            | Event::ConflictAnswered { .. }
            | Event::JobPaused(_)
            | Event::JobResumed(_)
            | Event::Undoable { .. }
            | Event::UndoGone(_)) => self.job_event(event, cx),
            // The window never searches or asks for drives yet, doesn't
            // wait on a file opening, and never watches jobs it didn't
            // start; only a picker's opening asks for the window state
            // (see `window_state`).
            Event::FolderUnreadable { .. }
            | Event::Drives(_)
            | Event::DriveDone { .. }
            | Event::Found { .. }
            | Event::SearchFailed { .. }
            | Event::Properties { .. }
            | Event::PropertiesUnreadable { .. }
            | Event::FileOpened
            | Event::FileNotOpened
            | Event::WindowState(_)
            | Event::JobRunning { .. }
            | Event::JobInterrupted { .. }
            | Event::Bookmarks(_)
            | Event::OpenWith(_)
            | Event::SizeMeasured { .. }
            | Event::SpaceMeasured { .. }
            | Event::SpaceUnreadable { .. } => {}
            Event::Trash(items) => self.trash_listed(items, cx),
            Event::Places(places) => {
                self.places = places;
                cx.emit(WindowEvent::Places);
                cx.notify();
            }
        }
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
            if status == CoreStatus::Gone {
                self.jobs_gone();
            }
            cx.emit(WindowEvent::Status(status));
            cx.notify();
        }
    }
}
