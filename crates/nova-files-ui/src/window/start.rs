//! Going to the Start page, and telling the core which folders a person
//! used.
//!
//! The Start page needs no listing, so going there arrives at once: the
//! folder asked for, if any, is dropped (its late answer is ignored), and
//! the folder on screen lets go of its listing, rows and thumbnails, so
//! nothing on Start acts on entries no longer shown.

use std::collections::HashMap;
use std::path::Path;

use gpui::{Context, UniformListScrollHandle};
use nova_files_api::Command;

use super::rows::release;
use super::{MainWindow, WindowEvent};
use crate::jump::TypeJump;
use crate::location::Location;

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
        cx.emit(WindowEvent::StartShown);
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
