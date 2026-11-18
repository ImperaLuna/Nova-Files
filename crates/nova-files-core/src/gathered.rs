//! Games and Projects: the folders each place gathers from, kept in
//! `$XDG_CONFIG_HOME/nova-files/gathered.toml` (see `file` for its text
//! and `store` for keeping it and telling clients).
//!
//! Nothing is found by itself, with one exception: when the file doesn't
//! exist yet, the first read writes one with the two usual folders, each
//! only if it exists: `~/Projects` for Projects and Steam's own library in
//! the data folder for Games. From then on the file is the only word, so a
//! folder the user removes stays removed. A file deleted later is written
//! again the next time the core starts.
//!
//! A Projects folder added without saying what it holds is guessed: one
//! with a project's files at its top (see [`PROJECT_MARKERS`]) is one
//! project, any other holds many.

mod file;
mod list;
mod store;

use std::fs;
use std::path::Path;

use nova_files_api::{GatheredFolder, Gathering, Holds};

pub use list::list;
pub use store::Store;

use crate::places::Home;

/// The file, in the config folder.
const FILE: &str = "gathered.toml";
/// What at the top of a folder makes it one project.
const PROJECT_MARKERS: [&str; 5] = [
    ".git",
    "flake.nix",
    "Cargo.toml",
    "package.json",
    "justfile",
];
/// The usual Projects folder, in the home folder.
const PROJECTS_IN_HOME: &str = "Projects";
/// Steam's own library, in the data folder.
const STEAM_IN_DATA: &str = "Steam/steamapps/common";
/// The name Steam's library shows under.
const STEAM_NAME: &str = "Steam";

/// What the folder at `path` holds in `place`, guessed from its top.
fn guess(place: Gathering, path: &Path) -> Holds {
    let is_project = place == Gathering::Projects
        && PROJECT_MARKERS
            .iter()
            .any(|marker| fs::symlink_metadata(path.join(marker)).is_ok());
    if is_project { Holds::One } else { Holds::Many }
}

/// The folders a new file starts with: the usual ones under `home` that
/// exist.
fn usual_folders(home: &Home) -> Vec<(Gathering, GatheredFolder)> {
    let projects = (
        Gathering::Projects,
        GatheredFolder {
            path: home.folder.join(PROJECTS_IN_HOME),
            holds: Holds::Many,
            name: None,
        },
    );
    let steam = (
        Gathering::Games,
        GatheredFolder {
            path: home.data.join(STEAM_IN_DATA),
            holds: Holds::Many,
            name: Some(STEAM_NAME.to_owned()),
        },
    );
    [projects, steam]
        .into_iter()
        .filter(|(_, folder)| fs::metadata(&folder.path).is_ok_and(|found| found.is_dir()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> Home {
        let root =
            std::env::temp_dir().join(format!("nova-gathered-{name}-{}", std::process::id()));
        fs::remove_dir_all(&root).ok();
        fs::create_dir_all(&root).unwrap();
        Home {
            folder: root.clone(),
            config: root.join("config"),
            state: root.join("state"),
            data: root.join("share"),
            drive_trashes: false,
        }
    }

    #[test]
    fn a_folder_with_a_projects_file_at_its_top_is_one_project() {
        let home = scratch("guess");
        for marker in PROJECT_MARKERS {
            let folder = home.folder.join(marker.replace('.', "dot-"));
            fs::create_dir_all(&folder).unwrap();
            assert_eq!(guess(Gathering::Projects, &folder), Holds::Many);
            fs::write(folder.join(marker), "").unwrap();
            assert_eq!(guess(Gathering::Projects, &folder), Holds::One, "{marker}");
            // A game library is never one game by guess.
            assert_eq!(guess(Gathering::Games, &folder), Holds::Many);
        }
        let deeper = home.folder.join("deeper");
        fs::create_dir_all(deeper.join("inner")).unwrap();
        fs::write(deeper.join("inner").join("Cargo.toml"), "").unwrap();
        assert_eq!(guess(Gathering::Projects, &deeper), Holds::Many);
        fs::remove_dir_all(&home.folder).ok();
    }

    #[test]
    fn only_the_usual_folders_that_exist_are_offered() {
        let home = scratch("usual");
        assert_eq!(usual_folders(&home), []);
        fs::create_dir_all(home.folder.join(PROJECTS_IN_HOME)).unwrap();
        let found = usual_folders(&home);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, Gathering::Projects);
        fs::create_dir_all(home.data.join(STEAM_IN_DATA)).unwrap();
        let found = usual_folders(&home);
        assert_eq!(found[1].0, Gathering::Games);
        assert_eq!(found[1].1.name.as_deref(), Some(STEAM_NAME));
        // A file where the folder should be is no folder.
        fs::remove_dir_all(home.folder.join(PROJECTS_IN_HOME)).unwrap();
        fs::write(home.folder.join(PROJECTS_IN_HOME), "").unwrap();
        assert_eq!(usual_folders(&home).len(), 1);
        fs::remove_dir_all(&home.folder).ok();
    }
}
