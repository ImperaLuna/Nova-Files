#![expect(clippy::unwrap_used, reason = "test setup should fail loudly")]

mod support;

use std::path::PathBuf;

use nova_files_index::{Change, Details, FileId, Flags, Index, Kind, Overlay, crawl};
use support::TempFolder;

const LIMIT: usize = 50;

const HOME: &[&str] = &[
    "projects/editor/src/components/render_frame.rs",
    "projects/editor/src/main.rs",
    "projects/editor/notes.md",
    "projects/viewer/src/render_pass.rs",
    "archive/2024/client/reports/quarterly_review.pdf",
    "archive/2023/client/reports/quarterly_review.pdf",
    "Documents/editor notes.txt",
    "Documents/render.txt",
];

struct Home {
    folder: TempFolder,
    _cache: TempFolder,
    index: Index,
}

impl Home {
    fn new() -> Self {
        let folder = TempFolder::new().with(HOME);
        let cache = TempFolder::new();
        let path = cache.path().join("index");
        crawl(folder.path(), || false).unwrap().save(&path).unwrap();
        let index = Index::open(&path).unwrap();
        Self {
            folder,
            _cache: cache,
            index,
        }
    }

    fn stages(&self, query: &str, overlay: &Overlay) -> Vec<Vec<PathBuf>> {
        self.index
            .search_now(query, LIMIT, overlay)
            .map(|hits| {
                hits.into_iter()
                    .map(|hit| {
                        hit.path
                            .strip_prefix(self.folder.path())
                            .unwrap()
                            .to_owned()
                    })
                    .collect()
            })
            .collect()
    }

    fn typed(&self, query: &str) -> Vec<PathBuf> {
        self.stages(query, &Overlay::new()).remove(0)
    }
}

fn paths(names: &[&str]) -> Vec<PathBuf> {
    names.iter().map(PathBuf::from).collect()
}

#[test]
fn a_query_with_slashes_follows_the_folders_in_order() {
    let home = Home::new();
    let frame = paths(&["projects/editor/src/components/render_frame.rs"]);
    assert_eq!(home.typed("editor/src/components/render"), frame);
    assert_eq!(home.typed("editor/render"), frame);
    assert_eq!(home.typed("EDITOR/comp/frame"), frame);
    assert_eq!(home.typed("components/editor/render"), paths(&[]));
    assert_eq!(
        home.typed("archive/2024/client/reports/quarterly"),
        paths(&["archive/2024/client/reports/quarterly_review.pdf"])
    );
}

#[test]
fn a_query_ending_in_a_slash_lists_what_the_folder_holds() {
    let home = Home::new();
    let mut inside = home.typed("viewer/");
    inside.sort();
    assert_eq!(
        inside,
        paths(&["projects/viewer/src", "projects/viewer/src/render_pass.rs"])
    );
}

#[test]
fn a_word_may_match_a_folder_when_another_matches_the_name() {
    let home = Home::new();
    assert_eq!(
        home.typed("editor render"),
        paths(&["projects/editor/src/components/render_frame.rs"])
    );
    assert_eq!(
        home.typed("render viewer"),
        paths(&["projects/viewer/src/render_pass.rs"])
    );
}

#[test]
fn a_hit_on_the_name_comes_before_a_hit_on_a_folder() {
    let home = Home::new();
    assert_eq!(
        home.typed("editor notes"),
        paths(&["Documents/editor notes.txt", "projects/editor/notes.md"])
    );
}

#[test]
fn a_folder_s_name_alone_does_not_bring_what_it_holds() {
    let home = Home::new();
    assert_eq!(
        home.typed("editor"),
        paths(&["projects/editor", "Documents/editor notes.txt"])
    );
    assert_eq!(
        home.typed("render"),
        paths(&[
            "Documents/render.txt",
            "projects/viewer/src/render_pass.rs",
            "projects/editor/src/components/render_frame.rs",
        ])
    );
}

#[test]
fn the_same_name_ranks_higher_nearer_the_top() {
    let home = Home::new();
    assert_eq!(
        home.typed("src"),
        paths(&["projects/editor/src", "projects/viewer/src"])
    );
    assert_eq!(
        home.typed("quarterly"),
        paths(&[
            "archive/2023/client/reports/quarterly_review.pdf",
            "archive/2024/client/reports/quarterly_review.pdf",
        ])
    );
}

#[test]
fn the_loose_stage_follows_folders_too() {
    let home = Home::new();
    let stages = home.stages("editor/redner_frame", &Overlay::new());
    assert_eq!(
        stages,
        [
            paths(&[]),
            paths(&["projects/editor/src/components/render_frame.rs"])
        ]
    );
}

#[test]
fn a_file_added_since_the_crawl_matches_along_its_path() {
    let home = Home::new();
    let mut overlay = Overlay::new();
    overlay.apply(&Change::Added {
        path: home.folder.path().join("projects/editor/src/shader.rs"),
        details: Details {
            kind: Kind::File,
            flags: Flags::default(),
            id: FileId {
                device: 0,
                inode: 0,
            },
            size: 0,
            modified: None,
            created: None,
        },
    });
    let shader = paths(&["projects/editor/src/shader.rs"]);
    assert_eq!(home.stages("editor/shader", &overlay).remove(0), shader);
    assert_eq!(home.stages("shader editor", &overlay).remove(0), shader);
    assert_eq!(home.stages("viewer/shader", &overlay).remove(0), paths(&[]));
}

#[test]
fn a_loose_name_match_inside_a_folder_the_word_also_matches_is_still_found() {
    let home = Home::new();
    let stages = home.stages("ed", &Overlay::new());
    let frame = PathBuf::from("projects/editor/src/components/render_frame.rs");
    assert!(!stages[0].contains(&frame));
    assert!(stages[1].contains(&frame));
}

fn moved(home: &Home, from: &str, to: &str) -> Overlay {
    let mut overlay = Overlay::new();
    overlay.apply(&Change::Moved {
        from: home.folder.path().join(from),
        to: home.folder.path().join(to),
    });
    overlay
}

#[test]
fn files_in_a_renamed_folder_match_its_new_name_and_not_the_old_one() {
    let home = Home::new();
    let overlay = moved(&home, "projects/editor", "projects/studio");
    assert_eq!(
        home.stages("studio/main", &overlay)[0],
        paths(&["projects/studio/src/main.rs"])
    );
    assert_eq!(
        home.stages("studio frame", &overlay)[0],
        paths(&["projects/studio/src/components/render_frame.rs"])
    );
    assert_eq!(home.stages("editor/main", &overlay)[0], paths(&[]));
    assert_eq!(
        home.stages("viewer/render", &overlay)[0],
        paths(&["projects/viewer/src/render_pass.rs"])
    );
}

#[test]
fn files_in_a_folder_moved_elsewhere_match_the_folders_now_above_them() {
    let home = Home::new();
    let overlay = moved(&home, "projects/viewer", "Documents/viewer");
    assert_eq!(
        home.stages("documents pass", &overlay)[0],
        paths(&["Documents/viewer/src/render_pass.rs"])
    );
    assert_eq!(home.stages("projects pass", &overlay)[0], paths(&[]));
}
