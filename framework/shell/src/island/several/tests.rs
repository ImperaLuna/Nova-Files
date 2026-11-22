use std::time::Duration;

use nova_files_api::Progress;

use super::*;
use crate::island::test_jobs::{fetch, store, words};

const TOTAL_BYTES: u64 = 10;
const HALF_THE_BYTES: u64 = TOTAL_BYTES / 2;

fn half_way(operation: Operation, start: Instant, secs: u64) -> Running {
    let mut job = Running::new(operation, start);
    let progress = |done_bytes| Progress {
        done_bytes,
        total_bytes: TOTAL_BYTES,
        done_items: 0,
        total_items: 1,
    };
    job.advanced(progress(0), start);
    job.advanced(progress(HALF_THE_BYTES), start + Duration::from_secs(secs));
    job
}

#[test]
fn the_folded_line_counts_the_jobs_and_says_when_the_last_ends() {
    let start = Instant::now();
    let now = start + Duration::from_secs(4);
    let quick = half_way(fetch(&["a"], "/b"), start, 2);
    let slow = half_way(fetch(&["c", "d"], "/b"), start, 4);

    assert_eq!(
        line(&[&quick, &slow], now, &words()),
        ("2 fetches".to_owned(), " · 4 s left".to_owned())
    );
    assert_eq!(fraction(&[&quick, &slow]), 0.5);
    let fresh = Running::new(fetch(&["e"], "/b"), start);
    assert_eq!(
        line(&[&quick, &fresh], now, &words()),
        ("2 fetches".to_owned(), String::new()),
        "not every one can say yet"
    );
    let moving = Running::new(store(&["x"], "/b"), start);
    assert_eq!(line(&[&moving, &moving], now, &words()).0, "2 stores");
    assert_eq!(line(&[&moving, &fresh], now, &words()).0, "2 jobs");
    assert_eq!(fraction(&[]), 0.0);
}

#[test]
fn each_row_names_what_it_works_on_and_its_seconds_left() {
    let start = Instant::now();
    let job = half_way(fetch(&["film.mkv"], "/home/a/Documents"), start, 6);
    let now = start + Duration::from_secs(6);

    assert_eq!(title(&job, now, &words()), "Into /home/a/Documents");
    assert_eq!(left(&job, now).as_deref(), Some("6 s"));
    let several = Running::new(fetch(&["a", "b"], "/b"), start);
    assert_eq!(title(&several, now, &words()), "Into /b");
    assert_eq!(left(&several, now), None);
    let empty = Running::new(Operation::EmptyTrash, start);
    assert_eq!(title(&empty, now, &words()), "Deleting");
}

#[test]
fn one_note_says_what_the_jobs_did_together() {
    let mut batch = Batch::default();
    let moved = store(&["x", "y"], "/b");
    let words = words();

    assert!(batch.keep(&fetch(&["a", "b", "c"], "/b"), Tally::default(), &words));
    assert!(batch.keep(
        &fetch(&["d", "e"], "/c"),
        Tally {
            replaced: 1,
            skipped: 1
        },
        &words
    ));
    assert!(batch.keep(&moved, Tally::default(), &words));
    assert!(!batch.keep(&Operation::EmptyTrash, Tally::default(), &words));
    assert_eq!(batch.ended.len(), 3);
    assert_eq!(
        batch.note(),
        "Fetched 4 items, stored 2 items, replaced 1, skipped 1"
    );
    assert!(batch.is_empty(), "let go");

    batch.keep(&moved, Tally::default(), &words);
    assert_eq!(batch.note(), "Stored 2 items");
    batch.keep(
        &fetch(&["a"], "/b"),
        Tally {
            replaced: 0,
            skipped: 1,
        },
        &words,
    );
    assert_eq!(batch.note(), "Nothing fetched, skipped 1");
    let skipped_both = Tally {
        replaced: 0,
        skipped: 2,
    };
    batch.keep(&moved, skipped_both, &words);
    batch.keep(&moved, skipped_both, &words);
    assert_eq!(batch.note(), "Nothing stored, skipped 4");
}
