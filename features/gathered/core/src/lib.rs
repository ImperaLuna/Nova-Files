#![forbid(unsafe_code)]

mod file;
mod list;
mod store;

use std::fs;
use std::path::Path;

use nova_files_disk::home::Home;
use nova_files_gathered_api::{GatheredFolder, Gathering, Holds};

pub use list::list;
pub use store::Store;

const FILE: &str = "gathered.toml";
const PROJECT_MARKERS: [&str; 5] = [
    ".git",
    "flake.nix",
    "Cargo.toml",
    "package.json",
    "justfile",
];
const PROJECTS_IN_HOME: &str = "Projects";
const STEAM_IN_DATA: &str = "Steam/steamapps/common";
const STEAM_NAME: &str = "Steam";

fn guess(place: Gathering, path: &Path) -> Holds {
    let is_project = place == Gathering::Projects
        && PROJECT_MARKERS
            .iter()
            .any(|marker| fs::symlink_metadata(path.join(marker)).is_ok());
    if is_project { Holds::One } else { Holds::Many }
}

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
    use nova_files_test_folder::TestFolder;

    fn scratch(name: &str) -> (TestFolder, Home) {
        let root = TestFolder::new(&format!("gathered-{name}"));
        let home = Home {
            folder: root.path().to_path_buf(),
            config: root.join("config"),
            state: root.join("state"),
            data: root.join("share"),
            drive_trashes: false,
        };
        (root, home)
    }

    #[test]
    fn a_folder_with_a_projects_file_at_its_top_is_one_project() {
        let (_held, home) = scratch("guess");
        for marker in PROJECT_MARKERS {
            let folder = home.folder.join(marker.replace('.', "dot-"));
            fs::create_dir_all(&folder).unwrap();
            assert_eq!(guess(Gathering::Projects, &folder), Holds::Many);
            fs::write(folder.join(marker), "").unwrap();
            assert_eq!(guess(Gathering::Projects, &folder), Holds::One, "{marker}");
            assert_eq!(guess(Gathering::Games, &folder), Holds::Many);
        }
        let deeper = home.folder.join("deeper");
        fs::create_dir_all(deeper.join("inner")).unwrap();
        fs::write(deeper.join("inner").join("Cargo.toml"), "").unwrap();
        assert_eq!(guess(Gathering::Projects, &deeper), Holds::Many);
    }

    #[test]
    fn only_the_usual_folders_that_exist_are_offered() {
        let (_held, home) = scratch("usual");
        assert_eq!(usual_folders(&home), []);
        fs::create_dir_all(home.folder.join(PROJECTS_IN_HOME)).unwrap();
        let found = usual_folders(&home);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, Gathering::Projects);
        fs::create_dir_all(home.data.join(STEAM_IN_DATA)).unwrap();
        let found = usual_folders(&home);
        assert_eq!(found[1].0, Gathering::Games);
        assert_eq!(found[1].1.name.as_deref(), Some(STEAM_NAME));
        fs::remove_dir_all(home.folder.join(PROJECTS_IN_HOME)).unwrap();
        fs::write(home.folder.join(PROJECTS_IN_HOME), "").unwrap();
        assert_eq!(usual_folders(&home).len(), 1);
    }
}
