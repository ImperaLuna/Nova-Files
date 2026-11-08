//! Searching a crawled home: stages, ranking, hidden rows, and changes
//! since the crawl.

#![expect(clippy::unwrap_used, reason = "test setup should fail loudly")]

mod support;

use std::path::PathBuf;

use nova_files_index::{
    Change, Details, FileId, Flags, Hit, Index, Kind, MatchKind, Overlay, crawl,
};
use support::TempFolder;

/// Enough for every test below.
const LIMIT: usize = 50;
const FEW: usize = 2;
/// How many hits "tax" has as typed.
const TAX_HITS: usize = 4;
/// More than a search has, so a search that never ends shows.
const STAGES_AND_ONE: usize = 3;

const HOME: &[&str] = &[
    "Documents/taxes 2024.pdf",
    "Documents/tax return.pdf",
    "Documents/syntax.txt",
    "Documents/report.pdf",
    "Pictures/Holiday/photos.txt",
    "Projects/nova-benchmark/",
    ".config/tax-tool/settings.toml",
    ".bashrc",
];

struct Home {
    folder: TempFolder,
    _cache: TempFolder,
    index: Index,
}

impl Home {
    fn new() -> Self {
        let home = TempFolder::new().with(HOME);
        let cache = TempFolder::new();
        let path = cache.path().join("index");
        crawl(home.path(), || false).unwrap().save(&path).unwrap();
        let index = Index::open(&path).unwrap();
        Self {
            folder: home,
            _cache: cache,
            index,
        }
    }

    /// Each stage's hits, as paths under home and kinds.
    fn stages(&self, query: &str, limit: usize) -> Vec<Vec<(PathBuf, MatchKind)>> {
        self.named(self.index.search(query, limit))
    }

    /// Like [`Home::stages`], after `changes`.
    fn stages_now(
        &self,
        query: &str,
        limit: usize,
        changes: &[Change],
    ) -> Vec<Vec<(PathBuf, MatchKind)>> {
        let mut overlay = Overlay::new();
        for change in changes {
            overlay.apply(change);
        }
        self.named(self.index.search_now(query, limit, &overlay))
    }

    fn named(&self, search: impl Iterator<Item = Vec<Hit>>) -> Vec<Vec<(PathBuf, MatchKind)>> {
        search
            .take(STAGES_AND_ONE)
            .map(|hits| {
                hits.into_iter()
                    .map(|hit| {
                        let path = hit.path.strip_prefix(self.folder.path()).unwrap();
                        (path.to_owned(), hit.kind)
                    })
                    .collect()
            })
            .collect()
    }

    fn at(&self, path: &str) -> PathBuf {
        self.folder.path().join(path)
    }

    fn added(&self, path: &str) -> Change {
        Change::Added {
            path: self.at(path),
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
        }
    }

    fn removed(&self, path: &str) -> Change {
        Change::Removed {
            path: self.at(path),
        }
    }

    fn moved(&self, from: &str, to: &str) -> Change {
        Change::Moved {
            from: self.at(from),
            to: self.at(to),
        }
    }
}

fn at(path: &str, kind: MatchKind) -> (PathBuf, MatchKind) {
    (PathBuf::from(path), kind)
}

#[test]
fn names_as_typed_come_first_then_loose_ones() {
    let home = Home::new();
    let stages = home.stages("tax", LIMIT);
    assert_eq!(
        stages,
        [
            vec![
                at("Documents/tax return.pdf", MatchKind::WordStart),
                at("Documents/taxes 2024.pdf", MatchKind::WordStart),
                at(".config/tax-tool", MatchKind::WordStart),
                at("Documents/syntax.txt", MatchKind::Contains),
            ],
            vec![],
        ]
    );
    let stages = home.stages("rprt", LIMIT);
    assert_eq!(
        stages,
        [vec![], vec![at("Documents/report.pdf", MatchKind::InOrder)]]
    );
}

#[test]
fn an_exact_name_ranks_first() {
    let home = Home::new();
    let first = home.stages("photos", LIMIT).remove(0);
    assert_eq!(first, [at("Pictures/Holiday/photos.txt", MatchKind::Exact)]);
    let first = home.stages("holiday", LIMIT).remove(0);
    assert_eq!(first, [at("Pictures/Holiday", MatchKind::Exact)]);
}

#[test]
fn words_match_in_any_order() {
    let home = Home::new();
    let first = home.stages("bench nova", LIMIT).remove(0);
    assert_eq!(first, [at("Projects/nova-benchmark", MatchKind::WordStart)]);
}

#[test]
fn a_typo_is_found_in_the_last_stage() {
    let home = Home::new();
    let stages = home.stages("phtoos", LIMIT);
    assert_eq!(
        stages,
        [
            vec![],
            vec![at("Pictures/Holiday/photos.txt", MatchKind::OneTypo)]
        ]
    );
}

#[test]
fn hidden_rows_rank_last_unless_the_query_starts_with_a_dot() {
    let home = Home::new();
    let first = home.stages("settings", LIMIT).remove(0);
    assert_eq!(
        first,
        [at(".config/tax-tool/settings.toml", MatchKind::Exact)]
    );
    let first = home.stages("ba", LIMIT).remove(0);
    assert_eq!(first.last(), Some(&at(".bashrc", MatchKind::WordStart)));
    let first = home.stages(".ba", LIMIT).remove(0);
    assert_eq!(first, [at(".bashrc", MatchKind::WordStart)]);
}

#[test]
fn each_stage_keeps_its_best_hits() {
    let home = Home::new();
    let stages = home.stages("tax", FEW);
    // The hits left out of the first stage don't come back in the second.
    assert_eq!(
        stages,
        [
            vec![
                at("Documents/tax return.pdf", MatchKind::WordStart),
                at("Documents/taxes 2024.pdf", MatchKind::WordStart),
            ],
            vec![],
        ]
    );
    let changes = [home.added("Documents/t-a-x.txt")];
    let stages = home.stages_now("tax", 1, &changes);
    assert_eq!(
        stages,
        [
            vec![at("Documents/tax return.pdf", MatchKind::WordStart)],
            vec![at("Documents/t-a-x.txt", MatchKind::InOrder)],
        ]
    );
    let changes = [
        home.added("Documents/tax notes.txt"),
        home.added("Documents/tax notes 2.txt"),
    ];
    let stages = home.stages_now("notes tax", 1, &changes);
    assert_eq!(
        stages,
        [
            vec![at("Documents/tax notes.txt", MatchKind::WordStart)],
            vec![]
        ]
    );
}

#[test]
fn a_limit_of_exactly_the_hits_keeps_them_all() {
    let home = Home::new();
    let first = home.stages("tax", TAX_HITS).remove(0);
    assert_eq!(first.len(), TAX_HITS);
}

#[test]
fn nothing_to_look_for_finds_nothing() {
    let home = Home::new();
    let none: Vec<Vec<(PathBuf, MatchKind)>> = Vec::new();
    assert_eq!(home.stages("   ", LIMIT), none);
    assert_eq!(home.stages("tax", 0), none);
    // Home itself is never a result.
    let root = home
        .folder
        .path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert_eq!(home.stages(&root, LIMIT), [vec![], vec![]]);
}

#[test]
fn without_changes_a_live_search_finds_the_same() {
    let home = Home::new();
    assert_eq!(home.stages_now("tax", FEW, &[]), home.stages("tax", FEW));
}

#[test]
fn removed_entries_are_left_out() {
    let home = Home::new();
    let first = home
        .stages_now("tax", FEW, &[home.removed("Documents/tax return.pdf")])
        .remove(0);
    // The next best fills the gap.
    assert_eq!(
        first,
        [
            at("Documents/taxes 2024.pdf", MatchKind::WordStart),
            at(".config/tax-tool", MatchKind::WordStart),
        ]
    );
}

#[test]
fn moved_entries_are_found_at_their_new_place() {
    let home = Home::new();
    let changes = [home.moved("Pictures/Holiday", "Documents/Holiday")];
    let first = home.stages_now("photos", LIMIT, &changes).remove(0);
    assert_eq!(
        first,
        [at("Documents/Holiday/photos.txt", MatchKind::Exact)]
    );
}

#[test]
fn renamed_entries_are_found_by_their_new_name() {
    let home = Home::new();
    let changes = [home.moved("Documents/report.pdf", "Documents/summary.pdf")];
    assert_eq!(home.stages_now("report", LIMIT, &changes), [vec![], vec![]]);
    let first = home.stages_now("summary", LIMIT, &changes).remove(0);
    assert_eq!(first, [at("Documents/summary.pdf", MatchKind::Exact)]);
}

#[test]
fn added_entries_are_found_in_both_stages() {
    let home = Home::new();
    let changes = [
        home.added("Documents/receipts.pdf"),
        home.added(".cache/receipts.log"),
    ];
    let stages = home.stages_now("receipt", LIMIT, &changes);
    assert_eq!(
        stages,
        [
            vec![
                at("Documents/receipts.pdf", MatchKind::WordStart),
                at(".cache/receipts.log", MatchKind::WordStart),
            ],
            vec![],
        ]
    );
    let stages = home.stages_now("rcpts", LIMIT, &changes);
    assert_eq!(
        stages,
        [
            vec![],
            vec![
                at("Documents/receipts.pdf", MatchKind::InOrder),
                at(".cache/receipts.log", MatchKind::InOrder),
            ]
        ]
    );
}

#[test]
fn an_entry_is_shown_once() {
    let home = Home::new();
    let changes = [home.added("Documents/report.pdf")];
    let first = home.stages_now("report", LIMIT, &changes).remove(0);
    assert_eq!(first, [at("Documents/report.pdf", MatchKind::Exact)]);
}
