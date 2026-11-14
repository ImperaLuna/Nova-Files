use std::path::{Path, PathBuf};

use super::*;

fn copy(names: &[&str]) -> Operation {
    Operation::Copy {
        sources: names
            .iter()
            .map(|name| Path::new("/a").join(name))
            .collect(),
        into: PathBuf::from("/b"),
    }
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
    let mut job = Running::new(copy(&["a", "b"]), start);
    // Looking at what to do: no totals yet.
    job.advanced(progress(0, 0, 0, 0), start + Duration::from_secs(5));
    assert_eq!(job.counted_from, None);
    let counted = start + Duration::from_secs(10);
    job.advanced(progress(100, 1000, 0, 2), counted);
    // Too soon to say.
    job.advanced(
        progress(200, 1000, 0, 2),
        counted + Duration::from_millis(500),
    );
    assert_eq!(job.time_left(counted + Duration::from_millis(500)), None);
    // 200 bytes in 2 s: the 700 left take 7 s.
    let now = counted + Duration::from_secs(2);
    job.advanced(progress(300, 1000, 1, 2), now);
    assert_eq!(job.time_left(now), Some(Duration::from_secs(7)));
    assert_eq!(job.line(now), "Copying 2 of 2 · 7 s left");
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
        job.line(start + Duration::from_secs(2)),
        "Moving 3 of 4 to Trash · 2 s left"
    );
}

#[test]
fn a_lone_source_goes_by_its_name_and_a_held_job_says_so() {
    let start = Instant::now();
    let mut job = Running::new(copy(&["Photos"]), start);
    assert_eq!(job.line(start), "Copying Photos");
    assert_eq!(job.fraction(), 0.0);
    job.waiting = true;
    assert_eq!(job.line(start), "Copying Photos · waiting");
    job.waiting = false;
    job.paused = true;
    assert_eq!(job.line(start), "Copying Photos · paused");
}

#[test]
fn before_the_count_several_sources_go_by_how_many() {
    let start = Instant::now();
    let job = Running::new(copy(&["a", "b", "c"]), start);
    assert_eq!(job.line(start), "Copying 3 items");
    let empty = Running::new(Operation::EmptyTrash, start);
    assert_eq!(empty.line(start), "Deleting");
}

#[test]
fn nothing_done_since_the_count_gives_no_time() {
    let start = Instant::now();
    let mut job = Running::new(copy(&["a", "b"]), start);
    job.advanced(progress(500, 1000, 1, 2), start);
    let later = start + Duration::from_secs(5);
    job.advanced(progress(500, 1000, 1, 2), later);
    assert_eq!(job.time_left(later), None);
    // A clock read before the count began gives none either.
    assert_eq!(
        job.time_left(start.checked_sub(Duration::from_secs(1)).unwrap()),
        None
    );
}
