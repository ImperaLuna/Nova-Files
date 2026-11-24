#![expect(clippy::unwrap_used, reason = "test setup should fail loudly")]

mod support;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use nova_files_index::{Index, Kept, crawl};
use support::TempFolder;

const LIMIT: usize = 500;

const WORDS: &[&str] = &[
    "render", "rendre", "rnder", "reader", "frame", "farme", "editor", "report", "main", "src",
];

const TYPED: &[&str] = &[
    "render_frame",
    "rendre frame",
    "editor/render",
    "src/",
    "report main",
    "farme",
];

struct Home {
    _folder: TempFolder,
    _cache: TempFolder,
    index: Index,
}

fn tree() -> Vec<String> {
    WORDS
        .iter()
        .flat_map(|folder| {
            WORDS.iter().flat_map(move |inner| {
                WORDS.iter().flat_map(move |first| {
                    [
                        format!("{folder}/{inner}/{first}.rs"),
                        format!("{folder}/{inner}/{first}_{folder}.txt"),
                        format!("{folder}/{first} {inner}.md"),
                    ]
                })
            })
        })
        .collect()
}

impl Home {
    fn new() -> Self {
        let files = tree();
        let files: Vec<&str> = files.iter().map(String::as_str).collect();
        let folder = TempFolder::new().with(&files);
        let cache = TempFolder::new();
        let path = cache.path().join("index");
        crawl(folder.path(), || false).unwrap().save(&path).unwrap();
        let index = Index::open(&path).unwrap();
        Self {
            _folder: folder,
            _cache: cache,
            index,
        }
    }

    fn fresh(&self, query: &str) -> (Vec<Vec<PathBuf>>, Option<Kept>) {
        let mut search = self.index.search(query, LIMIT);
        let stages = search.by_ref().map(paths).collect();
        (stages, search.keep())
    }

    fn from(&self, query: &str, kept: &Kept) -> (Vec<Vec<PathBuf>>, Option<Kept>) {
        let mut search = self.index.search(query, LIMIT).from(kept);
        let stages = search.by_ref().map(paths).collect();
        (stages, search.keep())
    }
}

fn paths(hits: Vec<nova_files_index::Hit>) -> Vec<PathBuf> {
    hits.into_iter().map(|hit| hit.path).collect()
}

fn prefixes(typed: &str) -> impl Iterator<Item = &str> {
    (1..=typed.len()).map(|end| &typed[..end])
}

#[test]
fn typing_on_from_the_last_matches_finds_what_a_fresh_search_finds() {
    let home = Home::new();
    for typed in TYPED {
        let mut last: Option<Kept> = None;
        for query in prefixes(typed) {
            let (fresh, fresh_kept) = home.fresh(query);
            let (reused, kept) = last
                .as_ref()
                .map_or_else(|| home.fresh(query), |kept| home.from(query, kept));
            assert!(reused == fresh, "{query:?} hits differ");
            assert!(kept == fresh_kept, "{query:?} kept rows differ");
            last = kept;
        }
    }
}

#[test]
fn matches_of_another_query_are_not_searched_from() {
    let home = Home::new();
    let (_, kept) = home.fresh("frame");
    let (fresh, _) = home.fresh("render");
    assert_ne!(fresh[0], Vec::<PathBuf>::new());
    assert_eq!(home.from("render", &kept.unwrap()).0, fresh);
}

#[test]
fn matches_inside_one_folder_are_not_searched_from_for_another() {
    let home = Home::new();
    let root = home.index.root().to_owned();
    let mut inside = home.index.search("rend", LIMIT).within(&root.join("src"));
    assert_ne!(inside.by_ref().next(), Some(Vec::new()));
    let kept = inside.keep().unwrap();
    let everywhere = home.index.search("rende", LIMIT).from(&kept);
    assert_eq!(
        everywhere.map(paths).collect::<Vec<_>>(),
        home.fresh("rende").0
    );
}

#[test]
fn a_stopped_search_keeps_nothing() {
    let home = Home::new();
    let stopped = AtomicBool::new(false);
    let stop = || stopped.load(Ordering::Relaxed);
    let mut search = home.index.search("render", LIMIT).stopping(&stop);
    stopped.store(true, Ordering::Relaxed);
    assert_eq!(search.next(), Some(Vec::new()));
    assert_eq!(search.keep(), None);
}

#[test]
fn a_search_left_before_its_last_stage_keeps_only_what_was_typed() {
    let home = Home::new();
    let mut search = home.index.search("rendr", LIMIT);
    let typed = search.next().unwrap();
    let kept = search.keep().unwrap();
    let (fresh, _) = home.fresh("rendre");
    let mut on = home.index.search("rendre", LIMIT).from(&kept);
    assert_eq!(on.next().map(paths), Some(fresh[0].clone()));
    assert_eq!(on.next().map(paths), Some(fresh[1].clone()));
    assert!(typed.len() <= LIMIT);
}
