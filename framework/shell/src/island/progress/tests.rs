use std::path::PathBuf;

use nova_files_api::{Conflict, ConflictKind};

use super::*;
use crate::island::test_jobs::{fetch, words};

fn fetching(names: &[&str]) -> Operation {
    fetch(names, "/b")
}

fn progress(done_bytes: u64, total_bytes: u64, done_items: u64, total_items: u64) -> Progress {
    Progress {
        done_bytes,
        total_bytes,
        done_items,
        total_items,
    }
}

#[test]
fn time_left_reads_in_seconds_minutes_and_hours_rounded_up() {
    assert_eq!(left_text(Duration::from_millis(200)), "1 s left");
    assert_eq!(left_text(Duration::from_secs(12)), "12 s left");
    assert_eq!(left_text(Duration::from_millis(59_500)), "1 min left");
    assert_eq!(left_text(Duration::from_secs(61)), "2 min left");
    assert_eq!(left_text(Duration::from_hours(1)), "1 h left");
    assert_eq!(left_text(Duration::from_mins(65)), "1 h 5 min left");
}

#[test]
fn time_left_comes_from_the_speed_since_the_totals_were_known() {
    let start = Instant::now();
    let mut job = Running::new(fetching(&["a", "b"]), start);
    job.advanced(progress(0, 0, 0, 0), start + Duration::from_secs(5));
    assert_eq!(job.counted_from, None);
    let counted = start + Duration::from_secs(10);
    job.advanced(progress(100, 1000, 0, 2), counted);
    job.advanced(
        progress(200, 1000, 0, 2),
        counted + Duration::from_millis(500),
    );
    assert_eq!(job.time_left(counted + Duration::from_millis(500)), None);
    let now = counted + Duration::from_secs(2);
    job.advanced(progress(300, 1000, 1, 2), now);
    assert_eq!(job.time_left(now), Some(Duration::from_secs(7)));
    assert_eq!(job.line(now, &words()), "Fetching 2 of 2 · 7 s left");
    assert!((job.fraction() - 0.3).abs() < 1e-6);
}

#[test]
fn a_job_without_bytes_goes_by_items() {
    let start = Instant::now();
    let mut job = Running::new(
        Operation::Trash {
            paths: vec![PathBuf::from("/a/x"), PathBuf::from("/a/y")],
        },
        start,
    );
    job.advanced(progress(0, 0, 0, 4), start);
    job.advanced(progress(0, 0, 2, 4), start + Duration::from_secs(2));
    assert_eq!(job.fraction(), 0.5);
    assert_eq!(
        job.line(start + Duration::from_secs(2), &words()),
        "Moving 3 of 4 to Trash · 2 s left"
    );
}

#[test]
fn a_lone_source_goes_by_its_name_and_a_held_job_says_so() {
    let start = Instant::now();
    let mut job = Running::new(fetching(&["Photos"]), start);
    assert_eq!(job.line(start, &words()), "Fetching Photos");
    assert_eq!(job.fraction(), 0.0);
    job.asking = Some(Conflict {
        kind: ConflictKind::FileExists,
        source: "/a/Photos".into(),
        target: "/b/Photos".into(),
        number: 1,
        more: 0,
    });
    assert_eq!(job.line(start, &words()), "Fetching Photos · waiting");
    job.asking = None;
    job.paused = true;
    assert_eq!(job.line(start, &words()), "Fetching Photos · paused");
}

#[test]
fn before_the_count_several_sources_go_by_how_many() {
    let start = Instant::now();
    let job = Running::new(fetching(&["a", "b", "c"]), start);
    assert_eq!(job.line(start, &words()), "Fetching 3 items");
    let empty = Running::new(Operation::EmptyTrash, start);
    assert_eq!(empty.line(start, &words()), "Deleting");
}

#[test]
fn nothing_done_since_the_count_gives_no_time() {
    let start = Instant::now();
    let mut job = Running::new(fetching(&["a", "b"]), start);
    job.advanced(progress(500, 1000, 1, 2), start);
    let later = start + Duration::from_secs(5);
    job.advanced(progress(500, 1000, 1, 2), later);
    assert_eq!(job.time_left(later), None);
    assert_eq!(
        job.time_left(start.checked_sub(Duration::from_secs(1)).unwrap()),
        None
    );
}

#[test]
fn time_left_shows_once_the_job_has_run_steady_long_enough() {
    let start = Instant::now();
    let mut job = Running::new(fetching(&["a"]), start);
    job.advanced(progress(0, 1000, 0, 1), start);
    let now = start + STEADY_AFTER;
    job.advanced(progress(500, 1000, 0, 1), now);
    assert_eq!(job.time_left(now), Some(STEADY_AFTER));
}

#[test]
fn a_job_of_one_item_from_several_sources_counts_them() {
    let start = Instant::now();
    let mut job = Running::new(fetching(&["a", "b"]), start);
    job.advanced(progress(0, 0, 0, 1), start);
    assert_eq!(job.line(start, &words()), "Fetching 2 items");
}
