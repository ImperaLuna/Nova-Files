//! Games and Projects as places: each lists the folders it gathers from
//! the folders the user chose (see the core's `gathered`), as one list
//! with each row in its own folder, like a search's files. Each row is a
//! folder; opening one goes there.
//!
//! The window asks the core for the gathered folders once it connects and
//! hears each change; it lists the place on screen again when the folders
//! it gathers from change, or what is inside them does. Going to a place
//! is like going to a folder: a `ListGathered` with a new id, and the
//! window moves once that id answers (see `events`).
//!
//! A gathered place has no folder: [`MainWindow::folder`] is `None` there,
//! so what acts on the folder on screen (New folder, Paste, a drop on the
//! list, Up, typing a path, Rename) has nothing to act on. Rows still
//! open, copy, cut and go to the Trash. A search from a place looks
//! everywhere (see `search`). The column after Name says where
//! each row is: its Library for Games (the gathered folder's name, else
//! its path), its Folder for Projects.

use std::mem;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::Context;
use nova_files_api::{Command, Gathered, Gathering, Holds};

use super::MainWindow;
use super::search::short_folder;
use crate::location::Location;

/// The head of the column after Name in Games.
pub const LIBRARY_LABEL: &str = "Library";

impl MainWindow {
    /// Goes to `place`, a new place in the history: asks the core for it,
    /// and the window moves there once it answers.
    pub fn open_gathered(&mut self, place: Gathering, cx: &mut Context<Self>) {
        self.forget_drive_to_open();
        self.history.leave();
        self.request_gathered(place);
        cx.notify();
    }

    /// Asks the core for what `place` gathers, leaving the history as it
    /// is: the window moves there once it answers, ending a search under
    /// way. Whether the ask was sent.
    pub(super) fn request_gathered(&mut self, place: Gathering) -> bool {
        self.ask_gathered();
        self.ask_listing(Location::Gathered(place), |id, order| {
            Command::ListGathered { id, place, order }
        })
    }

    /// Asks the core for `place` again or anew, leaving the history as it
    /// is; Start needs no ask. Whether the ask was sent.
    pub(super) fn request_place(&mut self, place: &Location) -> bool {
        match place {
            Location::Start => false,
            Location::Folder(folder) => self.request(folder),
            Location::Gathered(gathering) => self.request_gathered(*gathering),
        }
    }

    /// The gathered place whose listing is on screen, if one is; none
    /// while a search's files show.
    pub const fn gathered_shown(&self) -> Option<Gathering> {
        self.gathered.shown
    }

    /// The gathered place the window is at, if it is at one: on screen,
    /// or searched from.
    pub const fn gathered_here(&self) -> Option<Gathering> {
        match self.location() {
            Some(Location::Gathered(place)) => Some(*place),
            _ => None,
        }
    }

    /// Takes the gathered folders the core sent. A place on screen or on
    /// its way lists again when its own folders changed; the first ones
    /// sent are those its listing read.
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

    /// Asks the core for the gathered folders, once.
    pub(super) fn ask_gathered(&mut self) {
        if !self.gathered.asked
            && let Some(connection) = &self.connection
        {
            self.gathered.asked = connection.send(Command::Gathered).is_ok();
        }
    }

    /// What `place` gathers changed: listed again if it is on screen with
    /// nothing else on its way, or on its way itself, since that listing
    /// may have been read before the change.
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

    /// The library row `index` of Games is in (see [`Self::library_of`]).
    pub fn library_of_row(&self, index: usize) -> Option<String> {
        let listing = self.listing.as_ref()?;
        let entry = listing.entries.get(index)?;
        self.library_of(&listing.path_of(entry))
    }

    /// The library the game at `game` is in: the name of the gathered
    /// folder that brought it (the game's own, or the folder it is in),
    /// else that folder's path.
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

/// What a window knows of Games and Projects.
#[derive(Debug, Default)]
pub struct GatheredState {
    /// The folders they gather from, as the core last sent them.
    pub(super) folders: Arc<Gathered>,
    /// Whether the window asked the core for them.
    asked: bool,
    /// Whether the core sent them yet.
    pub(super) heard: bool,
    /// The place whose listing is on screen, if one is.
    pub(super) shown: Option<Gathering>,
    /// Folders to add once the core has sent what is gathered already.
    pub(super) to_gather: Option<(Gathering, Vec<PathBuf>)>,
    /// The add sent last, for its note.
    pub(super) adding: Option<Adding>,
}

/// Folders sent to the core to add to `place`.
#[derive(Debug)]
pub(super) struct Adding {
    pub(super) place: Gathering,
    /// The folders the core hasn't added yet.
    pub(super) asked: Vec<PathBuf>,
    /// The folders the core added, in the order it added them.
    pub(super) added: Vec<PathBuf>,
}

#[cfg(test)]
mod tests {
    use nova_files_api::Gathering;

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
