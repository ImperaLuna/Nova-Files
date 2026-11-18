//! Going to the Start page (and letting go of the folder on screen for
//! any place without one), its recent folders, and telling the core which
//! folders a person used.
//!
//! The Start page needs no listing, so going there arrives at once: the
//! folder asked for, if any, is dropped (its late answer is ignored), and
//! the folder on screen lets go of its listing, rows and thumbnails, so
//! nothing on Start acts on entries no longer shown.
//!
//! Each arrival asks the core for the recent folders again, leaving out
//! the ones the sidebar already shows, and so does a change to the places,
//! the pinned folders, or a card's folder that can't be read. Each ask has a new id and only
//! the newest one's answer is kept, whichever arrives last. Until it
//! comes, the page shows the folders from last time, so it draws at once.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::{Context, SharedString, UniformListScrollHandle};
use nova_files_api::{Command, RecentFolder, RecentId};

use super::rows::release;
use super::{MainWindow, WindowEvent};
use crate::jump::TypeJump;
use crate::location::Location;

/// The Start page's "Jump back in" cards: one large, four small.
pub const RECENT_CARDS: u32 = 5;

impl MainWindow {
    /// Goes to the Start page, a new place in the history.
    pub fn open_start(&mut self, cx: &mut Context<Self>) {
        self.history.leave();
        self.arrive_at_start(cx);
    }

    /// Shows the Start page, leaving the history's trip as it is.
    pub(super) fn arrive_at_start(&mut self, cx: &mut Context<Self>) {
        self.let_go_of_folder(cx);
        self.stop_measuring();
        self.history.arrived(&Location::Start);
        self.ask_for_recent();
        cx.emit(WindowEvent::StartShown);
        cx.notify();
    }

    /// Drops the folder asked for, if any, and lets go of the folder on
    /// screen, its rows, thumbnails and count, for a place that needs no
    /// listing.
    pub(super) fn let_go_of_folder(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        self.asked = None;
        self.requested = None;
        self.forget_rows();
        self.listing = None;
        self.details = HashMap::new();
        release(self.thumbnails.take_all(), None, cx);
        self.selection = None;
        self.scroll = UniformListScrollHandle::new();
        self.jump = TypeJump::default();
        self.band = None;
        self.counted = SharedString::default();
    }

    /// The recent folders the Start page shows.
    pub fn recent_folders(&self) -> &[RecentFolder] {
        &self.recent
    }

    /// Asks the core for the recent folders the Start page has room for,
    /// leaving out home, the places and the pinned folders.
    pub(super) fn ask_for_recent(&mut self) {
        let Some(connection) = &self.connection else {
            return;
        };
        self.recent_asked = next_ask(self.recent_asked);
        let id = self.recent_asked;
        let skip: Vec<PathBuf> = std::iter::once(self.home.clone())
            .chain(self.places.iter().map(|place| place.path.clone()))
            .chain(self.pinned.iter().map(|pinned| pinned.path.clone()))
            .collect();
        // A core gone ends the window's events, which shows it.
        connection
            .send(Command::RecentFolders {
                id,
                count: RECENT_CARDS,
                skip,
            })
            .ok();
    }

    /// Takes the recent folders the core sent for ask `id`, if it is the
    /// newest.
    pub(super) fn recent_arrived(
        &mut self,
        id: RecentId,
        recent: Vec<RecentFolder>,
        cx: &mut Context<Self>,
    ) {
        if id == self.recent_asked {
            self.recent = recent;
            cx.notify();
        }
    }

    /// Tells the core a person opened `folder` in a main window. What a
    /// picker shows is another app's choice, so it isn't counted.
    pub(super) fn used(&self, folder: &Path) {
        if self.pick.is_none()
            && let Some(connection) = &self.connection
        {
            connection.send(Command::FolderUsed(folder.to_owned())).ok();
        }
    }
}

/// The id of the ask after `id`.
const fn next_ask(id: RecentId) -> RecentId {
    RecentId(id.0 + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_ask_has_a_new_id() {
        assert_eq!(next_ask(RecentId(4)), RecentId(5));
    }
}
