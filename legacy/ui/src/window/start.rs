use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::{Context, SharedString, UniformListScrollHandle};
use nova_files_start_api::{Command, RecentFolder, RecentId};

use super::rows::release;
use super::{MainWindow, WindowEvent};
use crate::jump::TypeJump;
use crate::location::Location;

pub const RECENT_CARDS: u32 = 5;

impl MainWindow {
    pub fn open_start(&mut self, cx: &mut Context<Self>) {
        self.history.leave();
        self.arrive_at_start(cx);
    }

    pub(super) fn arrive_at_start(&mut self, cx: &mut Context<Self>) {
        self.let_go_of_folder(cx);
        self.stop_measuring();
        self.history.arrived(&Location::Start);
        self.ask_for_recent();
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
        self.details = HashMap::new();
        release(self.thumbnails.take_all(), None, cx);
        self.selection = None;
        self.scroll = UniformListScrollHandle::new();
        self.jump = TypeJump::default();
        self.band = None;
        self.counted = SharedString::default();
        self.search_left(cx);
    }

    pub fn recent_folders(&self) -> &[RecentFolder] {
        &self.recent
    }

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
        connection
            .send(Command::RecentFolders {
                id,
                count: RECENT_CARDS,
                skip,
            })
            .ok();
    }

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

    pub(super) fn used(&self, folder: &Path) {
        if !self.picker()
            && let Some(connection) = &self.connection
        {
            connection.send(Command::FolderUsed(folder.to_owned())).ok();
        }
    }
}

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
