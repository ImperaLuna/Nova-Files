use std::cell::Cell;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::time::{Duration, SystemTime};

use nova_files_api::SortKey;
use nova_files_test_folder::TestFolder;

use super::*;

struct Scratch(TestFolder);

impl Scratch {
    fn new(name: &str) -> Self {
        Self(TestFolder::new(&format!("gathered-list-{name}")))
    }

    fn at(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn folders(&self, names: &[&str]) {
        for name in names {
            fs::create_dir_all(self.at(name)).unwrap();
        }
    }
}

fn many(path: PathBuf) -> GatheredFolder {
    GatheredFolder {
        path,
        holds: Holds::Many,
        name: None,
    }
}

fn one(path: PathBuf) -> GatheredFolder {
    GatheredFolder {
        path,
        holds: Holds::One,
        name: None,
    }
}

fn rows(root: &Path, folders: &[GatheredFolder], order: Order) -> Vec<(String, String)> {
    let gathered = gather(folders, HiddenFiles::Left, order, || false).unwrap();
    gathered
        .iter()
        .enumerate()
        .map(|(index, (entry, home))| {
            assert_eq!(entry.id, EntryId(u32::try_from(index).unwrap()));
            assert_eq!(entry.kind, EntryKind::Folder);
            (
                entry.name.to_string_lossy().into_owned(),
                home.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            )
        })
        .collect()
}

fn row(name: &str, home: &str) -> (String, String) {
    (name.to_owned(), home.to_owned())
}

#[test]
fn folders_inside_and_one_projects_list_together_a_to_z() {
    let scratch = Scratch::new("together");
    scratch.folders(&["Projects/nova", "Projects/Bench", "Skills", "work/zeta"]);
    fs::write(scratch.at("Projects/README.md"), "").unwrap();
    let folders = [
        many(scratch.at("Projects")),
        one(scratch.at("Skills")),
        many(scratch.at("work")),
    ];
    assert_eq!(
        rows(scratch.0.path(), &folders, Order::default()),
        [
            row("Bench", "Projects"),
            row("nova", "Projects"),
            row("Skills", ""),
            row("zeta", "work"),
        ]
    );
    let backwards = Order {
        key: SortKey::Name,
        descending: true,
    };
    assert_eq!(
        rows(scratch.0.path(), &folders, backwards)[0],
        row("zeta", "work")
    );
}

#[test]
fn a_folder_reached_twice_shows_once() {
    let scratch = Scratch::new("twice");
    scratch.folders(&["Projects/nova"]);
    let folders = [
        many(scratch.at("Projects")),
        one(scratch.at("Projects/nova")),
    ];
    assert_eq!(
        rows(scratch.0.path(), &folders, Order::default()),
        [row("nova", "Projects")]
    );
}

#[test]
fn missing_and_unreadable_folders_add_nothing() {
    let scratch = Scratch::new("missing");
    scratch.folders(&["Projects/nova"]);
    fs::write(scratch.at("a-file"), "").unwrap();
    let folders = [
        many(scratch.at("gone")),
        one(scratch.at("also-gone")),
        many(scratch.at("a-file")),
        one(scratch.at("a-file")),
        many(scratch.at("Projects")),
    ];
    assert_eq!(
        rows(scratch.0.path(), &folders, Order::default()),
        [row("nova", "Projects")]
    );
}

#[test]
fn hidden_folders_follow_the_setting_and_links_to_folders_count() {
    let scratch = Scratch::new("hidden");
    scratch.folders(&["Projects/.cache", "Projects/real", "elsewhere"]);
    symlink(scratch.at("elsewhere"), scratch.at("Projects/linked")).unwrap();
    let folders = [many(scratch.at("Projects"))];
    assert_eq!(
        rows(scratch.0.path(), &folders, Order::default()),
        [row("linked", "Projects"), row("real", "Projects")]
    );
    let shown = gather(&folders, HiddenFiles::Shown, Order::default(), || false).unwrap();
    assert_eq!(shown.len(), 3);
    scratch.folders(&[".dotfiles"]);
    let dotfiles = [one(scratch.at(".dotfiles"))];
    assert_eq!(rows(scratch.0.path(), &dotfiles, Order::default()), []);
    let shown = gather(&dotfiles, HiddenFiles::Shown, Order::default(), || false).unwrap();
    assert_eq!(shown.len(), 1);
}

#[test]
fn newest_first_reads_each_folder_where_it_is() {
    let scratch = Scratch::new("modified");
    scratch.folders(&["a/old", "b/new"]);
    let aged = |path: PathBuf, age: u64| {
        fs::File::open(path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(age))
            .unwrap();
    };
    aged(scratch.at("a/old"), 600);
    aged(scratch.at("b/new"), 10);
    let newest = Order {
        key: SortKey::Modified,
        descending: true,
    };
    assert_eq!(
        rows(
            scratch.0.path(),
            &[many(scratch.at("a")), many(scratch.at("b"))],
            newest
        ),
        [row("new", "b"), row("old", "a")]
    );
}

#[test]
fn a_cancel_stops_before_the_next_folder() {
    let scratch = Scratch::new("cancel");
    scratch.folders(&["a/x", "b/y"]);
    let asked = Cell::new(0);
    let cancelled = || {
        asked.set(asked.get() + 1);
        asked.get() > 1
    };
    let folders = [many(scratch.at("a")), many(scratch.at("b"))];
    assert!(gather(&folders, HiddenFiles::Left, Order::default(), cancelled).is_none());
}

#[test]
fn steam_tools_in_a_library_are_not_listed() {
    let scratch = Scratch::new("steam-tools");
    scratch.folders(&[
        "common/Celeste/_CommonRedist",
        "common/Proton 10.0",
        "common/SteamLinuxRuntime_sniper",
        "common/Steamworks Shared/_CommonRedist",
        "common/wallpaper_engine",
    ]);
    fs::write(scratch.at("common/Celeste/Celeste.exe"), "").unwrap();
    fs::write(scratch.at("common/Proton 10.0/toolmanifest.vdf"), "").unwrap();
    fs::write(
        scratch.at("common/SteamLinuxRuntime_sniper/toolmanifest.vdf"),
        "",
    )
    .unwrap();
    assert_eq!(
        rows(
            scratch.0.path(),
            &[many(scratch.at("common"))],
            Order::default()
        ),
        [row("Celeste", "common"), row("wallpaper_engine", "common")]
    );
}
