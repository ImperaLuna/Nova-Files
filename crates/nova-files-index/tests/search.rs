//! Searching a crawled home: stages, ranking, hidden rows.

#![expect(clippy::unwrap_used, reason = "test setup should fail loudly")]

mod support;

use std::path::PathBuf;

use nova_files_index::{Hit, Index, MatchKind, crawl};
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
        self.index
            .search(query, limit)
            .take(STAGES_AND_ONE)
            .map(|hits| hits.into_iter().map(|hit| self.named(hit)).collect())
            .collect()
    }

    fn named(&self, hit: Hit) -> (PathBuf, MatchKind) {
        let path = self.index.path(hit.row).unwrap();
        (
            path.strip_prefix(self.folder.path()).unwrap().to_owned(),
            hit.kind,
        )
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
    let first = home.stages("tax", FEW).remove(0);
    assert_eq!(
        first,
        [
            at("Documents/tax return.pdf", MatchKind::WordStart),
            at("Documents/taxes 2024.pdf", MatchKind::WordStart),
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
    // A lone accent folds to nothing, so it is no word to look for.
    assert_eq!(home.stages("\u{301}", LIMIT), none);
    assert_eq!(home.stages("tax \u{301}", LIMIT), home.stages("tax", LIMIT));
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
