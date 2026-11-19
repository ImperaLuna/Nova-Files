use std::mem;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::Context;
use nova_files_gathered_api::{Command, Event, Gathered, Gathering, Holds};

use super::MainWindow;
use super::search::short_folder;
use crate::location::Location;

pub const LIBRARY_LABEL: &str = "Library";

impl MainWindow {
    pub(super) fn gathered_event(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Gathered(gathered) => self.gathered_arrived(gathered, cx),
            Event::GatheredChanged(place) => self.gathered_changed(place),
        }
    }

    pub fn open_gathered(&mut self, place: Gathering, cx: &mut Context<Self>) {
        self.went_elsewhere();
        self.history.leave();
        self.request_gathered(place);
        cx.notify();
    }

    pub(super) fn request_gathered(&mut self, place: Gathering) -> bool {
        self.ask_gathered();
        self.ask_listing(Location::Gathered(place), |id, order| {
            Command::ListGathered { id, place, order }
        })
    }

    pub(super) fn request_place(&mut self, place: &Location) -> bool {
        match place {
            Location::Start => false,
            Location::Page(page) => self.request_listed(page.clone()),
            Location::Folder(folder) => self.request(folder),
            Location::Gathered(gathering) => self.request_gathered(*gathering),
        }
    }

    pub const fn gathered_shown(&self) -> Option<Gathering> {
        self.gathered.shown
    }

    pub const fn gathered_here(&self) -> Option<Gathering> {
        match self.location() {
            Some(Location::Gathered(place)) => Some(*place),
            _ => None,
        }
    }

    pub(super) fn gathered_arrived(&mut self, gathered: Arc<Gathered>, cx: &mut Context<Self>) {
        let before = mem::replace(&mut self.gathered.folders, gathered);
        if mem::replace(&mut self.gathered.heard, true) {
            for place in Gathering::ALL {
                if before.of(place) != self.gathered.folders.of(place) {
                    self.gathered_changed(place);
                }
            }
            self.note_added(cx);
        } else if let Some((place, paths)) = self.gathered.to_gather.take() {
            self.gather(place, paths, cx);
        }
        cx.notify();
    }

    pub(super) fn ask_gathered(&mut self) {
        if !self.gathered.asked
            && let Some(connection) = &self.connection
        {
            self.gathered.asked = connection.send(Command::Gathered).is_ok();
        }
    }

    pub fn gathered_changed(&mut self, place: Gathering) {
        let wanted = Some(Location::Gathered(place));
        let again = if self.waiting() {
            self.requested == wanted
        } else {
            self.gathered_shown() == Some(place)
        };
        if again {
            self.request_gathered(place);
        }
    }

    pub fn library_of_row(&self, index: usize) -> Option<String> {
        let listing = self.listing.as_ref()?;
        let entry = listing.entries.get(index)?;
        self.library_of(&listing.path_of(entry))
    }

    pub(crate) fn library_of(&self, game: &Path) -> Option<String> {
        let folder = game.parent()?;
        let libraries = self.gathered.folders.of(Gathering::Games);
        let library = libraries
            .iter()
            .find(|library| library.holds == Holds::One && library.path == game)
            .or_else(|| {
                libraries
                    .iter()
                    .find(|library| library.holds == Holds::Many && library.path == folder)
            });
        Some(
            library
                .and_then(|library| library.name.clone())
                .unwrap_or_else(|| short_folder(folder, &self.home)),
        )
    }
}

#[derive(Debug, Default)]
pub struct GatheredState {
    pub(super) folders: Arc<Gathered>,
    asked: bool,
    pub(super) heard: bool,
    pub(super) shown: Option<Gathering>,
    pub(super) to_gather: Option<(Gathering, Vec<PathBuf>)>,
    pub(super) adding: Option<Adding>,
}

#[derive(Debug)]
pub(super) struct Adding {
    pub(super) place: Gathering,
    pub(super) asked: Vec<PathBuf>,
    pub(super) added: Vec<PathBuf>,
}

#[cfg(test)]
mod tests {
    use nova_files_gathered_api::Gathering;

    use crate::assets::Icon;
    use crate::places::{GAMES_LABEL, PROJECTS_LABEL, gathering_look};

    #[test]
    fn each_place_has_its_own_name_and_icon() {
        assert_eq!(
            gathering_look(Gathering::Projects),
            (PROJECTS_LABEL, Icon::Projects)
        );
        assert_eq!(gathering_look(Gathering::Games), (GAMES_LABEL, Icon::Games));
    }
}
