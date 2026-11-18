//! Gathered folders: Games and Projects, the places that gather folders
//! from several spots into one list.
//!
//! The core keeps them in `gathered.toml` in its config folder, which
//! people may also write by hand or from Nix. A client asks for them with
//! [`Command::Gathered`] and changes them with [`Command::ChangeGathered`];
//! every client that asked hears [`Event::Gathered`] after each change,
//! from any client or the file.
//!
//! [`Command::Gathered`]: crate::Command::Gathered
//! [`Command::ChangeGathered`]: crate::Command::ChangeGathered
//! [`Event::Gathered`]: crate::Event::Gathered

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A place that gathers folders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Gathering {
    Projects,
    Games,
}

impl Gathering {
    /// Both, in the order Places shows them.
    pub const ALL: [Self; 2] = [Self::Projects, Self::Games];
}

/// What one gathered folder brings to its place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Holds {
    /// Each folder inside it is one row: a folder of projects, or a game
    /// library.
    #[default]
    Many,
    /// The folder itself is one row: one project.
    One,
}

/// One folder a place gathers from. It may be missing (on a drive not
/// plugged in); it stays until the user removes it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GatheredFolder {
    pub path: PathBuf,
    pub holds: Holds,
    /// The name the user gave it, shown where its rows came from; without
    /// one its path shows.
    pub name: Option<String>,
}

/// The folders each place gathers from, in the user's order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Gathered {
    pub projects: Arc<[GatheredFolder]>,
    pub games: Arc<[GatheredFolder]>,
}

impl Gathered {
    /// The folders `place` gathers from.
    pub fn of(&self, place: Gathering) -> &[GatheredFolder] {
        match place {
            Gathering::Projects => &self.projects,
            Gathering::Games => &self.games,
        }
    }

    /// Whether `place` already gathers from `path`.
    pub fn has(&self, place: Gathering, path: &Path) -> bool {
        self.of(place).iter().any(|folder| folder.path == path)
    }
}

/// One change to the gathered folders. A change naming a folder the place
/// doesn't have (another window removed it first) does nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatheredChange {
    /// Gather from the folder at `path`, before the folder now at position
    /// `at`, or last when `at` is `None` or past the end. Without `holds`
    /// the core guesses: a Projects folder with a project's files at its
    /// top (`.git`, `Cargo.toml` and the like) is one project, anything
    /// else holds many. A folder already gathered stays as it is.
    Add {
        place: Gathering,
        path: PathBuf,
        holds: Option<Holds>,
        at: Option<u32>,
    },
    /// Stop gathering from the folder at `path`.
    Remove { place: Gathering, path: PathBuf },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_looks_only_in_the_place_asked() {
        let folder = GatheredFolder {
            path: PathBuf::from("/home/a/Projects"),
            holds: Holds::default(),
            name: None,
        };
        let gathered = Gathered {
            projects: Arc::from([folder]),
            games: Arc::from([]),
        };
        assert!(gathered.has(Gathering::Projects, Path::new("/home/a/Projects")));
        assert!(!gathered.has(Gathering::Projects, Path::new("/home/a/Other")));
        assert!(!gathered.has(Gathering::Games, Path::new("/home/a/Projects")));
    }
}
