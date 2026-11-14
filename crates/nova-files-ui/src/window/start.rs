//! Going to the Start page, its recent folders, and telling the core which
//! folders a person used.
//!
//! The Start page needs no listing, so going there arrives at once: the
//! folder asked for, if any, is dropped (its late answer is ignored), and
//! the folder on screen lets go of its listing, rows and thumbnails, so
//! nothing on Start acts on entries no longer shown.
//!
//! Each arrival asks the core for the recent folders again, leaving out
//! the ones the sidebar already shows. Until the answer comes, the page
//! shows the ones from last time, so it draws at once.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::{Context, UniformListScrollHandle};
use nova_files_api::{Command, RecentFolder};

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
        self.history.arrived(&Location::Start);
        self.ask_for_recent();
        cx.emit(WindowEvent::StartShown);
        cx.notify();
    }

    /// The recent folders the Start page shows.
    pub fn recent_folders(&self) -> &[RecentFolder] {
        &self.recent
    }

    /// Asks the core for the recent folders the Start page has room for,
    /// leaving out home and the places.
    pub(super) fn ask_for_recent(&self) {
        let Some(connection) = &self.connection else {
            return;
        };
        let skip: Vec<PathBuf> = std::iter::once(self.home.clone())
            .chain(self.places.iter().map(|place| place.path.clone()))
            .collect();
        // A core gone ends the window's events, which shows it.
        connection
            .send(Command::RecentFolders {
                count: RECENT_CARDS,
                skip,
            })
            .ok();
    }

    /// Takes the recent folders the core sent.
    pub(super) fn recent_arrived(&mut self, recent: Vec<RecentFolder>, cx: &mut Context<Self>) {
        self.recent = recent;
        cx.notify();
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
