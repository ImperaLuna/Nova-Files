use nova_files_test_folder::TestFolder;

use super::*;
use crate::build::Built;
use crate::{Details, FileId, Flags, Kind};

const FILES: u32 = 100_000;
const NEEDLE_EVERY: u32 = 1000;
const LIMIT: usize = 50;

fn facts(kind: Kind) -> Details {
    Details {
        kind,
        flags: Flags::default(),
        id: FileId {
            device: 1,
            inode: 1,
        },
        size: 0,
        modified: None,
        created: None,
    }
}

fn name_of(file: u32) -> String {
    if file.is_multiple_of(NEEDLE_EVERY) {
        format!("needle-{file}.txt")
    } else {
        format!("straw-{file}.txt")
    }
}

fn haystack(name: &str) -> (TestFolder, Index) {
    let folder = TestFolder::new(name);
    let mut built = Built::new(None);
    let root = built.push(None, b"/hay", facts(Kind::Folder)).unwrap();
    for file in 0..FILES {
        built
            .push(Some(root), name_of(file).as_bytes(), facts(Kind::File))
            .unwrap();
    }
    built.close(root);
    let path = folder.join("index");
    built.save(&path).unwrap();
    let index = Index::open(&path).unwrap();
    (folder, index)
}

fn shortest_needles() -> Vec<PathBuf> {
    let mut needles: Vec<String> = (0..FILES)
        .step_by(NEEDLE_EVERY as usize)
        .map(name_of)
        .collect();
    needles.sort_by_key(String::len);
    needles.truncate(LIMIT);
    needles
        .iter()
        .map(|name| Path::new("/hay").join(name))
        .collect()
}

#[test]
fn a_scan_split_over_threads_keeps_the_best_hits_in_order() {
    let (_folder, index) = haystack("search-threads");
    assert!(FILES as usize >= 2 * scan::ROWS_PER_THREAD_MIN);
    let mut stages = index.search("needle", LIMIT);
    let typed: Vec<PathBuf> = stages
        .next()
        .unwrap()
        .into_iter()
        .map(|hit| hit.path)
        .collect();
    assert_eq!(typed, shortest_needles());
    assert_eq!(stages.next(), Some(Vec::new()));
}

#[test]
fn the_loose_stage_leaves_out_what_the_typed_stage_found() {
    let (_folder, index) = haystack("search-loose");
    let mut stages = index.search("nedle", LIMIT);
    assert_eq!(stages.next(), Some(Vec::new()));
    let loose = stages.next().unwrap();
    let paths: Vec<PathBuf> = loose.iter().map(|hit| hit.path.clone()).collect();
    assert_eq!(paths, shortest_needles());
    assert!(loose.iter().all(|hit| hit.kind == MatchKind::InOrder));
}

#[test]
fn letters_say_which_names_cannot_match() {
    let straw = fold::letters(b"straw-1.txt");
    let short_by = |word: &[u8]| (fold::letters(word) & !straw).count_ones();
    assert_eq!(short_by(b"straw"), 0);
    assert_eq!(short_by(b"st-1."), 0);
    assert_eq!(short_by(b"strew"), 1);
    assert_eq!(short_by(b"needle"), 4);
    assert_eq!(short_by(b"a b"), 2);
}
