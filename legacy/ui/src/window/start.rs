use std::collections::HashMap;
use std::path::Path;

use gpui::{App, Context, SharedString, UniformListScrollHandle};

use super::{MainWindow, WindowEvent};
use crate::jump::TypeJump;
use crate::location::Location;

impl MainWindow {
    pub fn open_start(&mut self, cx: &mut Context<Self>) {
        self.history.leave();
        self.arrive_at_start(cx);
    }

    pub(super) fn arrive_at_start(&mut self, cx: &mut Context<Self>) {
        self.let_go_of_folder(cx);
        self.history.arrived(&Location::Start);
        cx.emit(WindowEvent::StartShown);
        cx.notify();
    }

    pub(super) fn let_go_of_folder(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        self.asked = None;
        self.requested = None;
        self.forget_rows();
        self.listing = None;
        self.gathered.shown = None;
        self.listed_page = None;
        self.details = HashMap::new();
        self.selection = None;
        self.scroll = UniformListScrollHandle::new();
        self.jump = TypeJump::default();
        self.band = None;
        self.counted = SharedString::default();
        self.search_left(cx);
    }

    pub(super) fn used(&self, folder: &Path, cx: &mut App) {
        if self.picker() {
            return;
        }
        self.parts.used(folder, cx);
    }
}
