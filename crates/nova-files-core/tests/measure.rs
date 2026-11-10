//! Size scans through the real core, against real temp folders: the client
//! that asked hears the totals, a stopped scan goes quiet, and another
//! client hears nothing.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

#[expect(dead_code, reason = "the job helpers; scans need only the bench")]
mod common;

use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use nova_files_api::{Command, Event, MeasureId, ScanState, Size};

use common::{Bench, Client};

/// Folders in a tree big enough that a scan is still running when the
/// client stops it.
const MANY: usize = 2000;
/// How long a test listens for an answer that should never come.
const QUIET: Duration = Duration::from_millis(300);

/// Waits for scan `id` to be done, skipping its reports so far, and
/// checks they never went down.
fn measured(client: &Client, id: MeasureId) -> Size {
    let mut last = Size::default();
    loop {
        match client.next() {
            Event::SizeMeasured {
                id: got,
                size,
                state,
            } if got == id => {
                assert!(size.files >= last.files && size.bytes >= last.bytes);
                last = size;
                match state {
                    ScanState::Counting => {}
                    ScanState::Done => return size,
                    ScanState::CutShort => panic!("scan {id:?} was cut short"),
                }
            }
            other => panic!("expected scan {id:?}'s sizes, got {other:?}"),
        }
    }
}

fn measure(client: &Client, id: MeasureId, paths: Vec<PathBuf>) {
    client.send(Command::Measure { id, paths });
}

#[test]
fn a_scan_adds_up_all_the_paths_it_was_given() {
    let bench = Bench::new("measure-paths");
    fs::create_dir_all(bench.at("a/inner")).unwrap();
    fs::write(bench.at("a/inner/one"), [0; 300]).unwrap();
    fs::write(bench.at("two"), [0; 20]).unwrap();
    let client = bench.client();

    let id = MeasureId(3);
    measure(&client, id, vec![bench.at("a"), bench.at("two")]);
    let size = measured(&client, id);

    assert_eq!(
        (size.bytes, size.files, size.folders, size.unreadable),
        (320, 2, 2, 0)
    );
}

#[test]
fn a_path_that_is_not_there_counts_as_unreadable() {
    let bench = Bench::new("measure-missing");
    let client = bench.client();

    let id = MeasureId(1);
    measure(&client, id, vec![bench.at("gone")]);

    assert_eq!(
        measured(&client, id),
        Size {
            unreadable: 1,
            ..Size::default()
        }
    );
}

/// Waits until the core says nothing for [`QUIET`], allowing only
/// unfinished reports of scan `id`.
fn only_reports_of(client: &Client, id: MeasureId) {
    loop {
        match client.events.recv_timeout(QUIET) {
            Ok(Event::SizeMeasured { id: got, state, .. }) => {
                assert!(got == id && state == ScanState::Counting);
            }
            Ok(other) => panic!("unexpected {other:?}"),
            Err(RecvTimeoutError::Timeout) => break,
            Err(error) => panic!("{error}"),
        }
    }
}

fn big_tree(bench: &Bench) -> PathBuf {
    for folder in 0..MANY {
        fs::create_dir_all(bench.at(&format!("big/{folder}/inside"))).unwrap();
    }
    bench.at("big")
}

#[test]
fn a_stopped_scan_says_nothing_more() {
    let bench = Bench::new("measure-stop");
    let big = big_tree(&bench);
    let client = bench.client();

    let id = MeasureId(1);
    measure(&client, id, vec![big]);
    client.send(Command::StopMeasure(id));

    // Reports sent before the stop may still arrive; the last one never.
    only_reports_of(&client, id);
}

#[test]
fn a_newer_scan_stops_the_one_before() {
    let bench = Bench::new("measure-newer");
    let big = big_tree(&bench);
    fs::write(bench.at("small"), [0; 7]).unwrap();
    let client = bench.client();

    let (first, second) = (MeasureId(1), MeasureId(2));
    measure(&client, first, vec![big]);
    measure(&client, second, vec![bench.at("small")]);

    loop {
        match client.next() {
            Event::SizeMeasured { id, state, .. } if id == first => {
                assert_eq!(state, ScanState::Counting);
            }
            Event::SizeMeasured { id, size, state } if id == second => {
                if state == ScanState::Done {
                    assert_eq!(size.bytes, 7);
                    break;
                }
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    only_reports_of(&client, first);
}

#[test]
fn stopping_an_older_scan_leaves_the_newer_one_running() {
    let bench = Bench::new("measure-stop-old");
    fs::write(bench.at("file"), [0; 3]).unwrap();
    let client = bench.client();

    let (old, new) = (MeasureId(1), MeasureId(2));
    measure(&client, new, vec![bench.at("file")]);
    client.send(Command::StopMeasure(old));

    assert_eq!(measured(&client, new).bytes, 3);
}

#[test]
fn paths_inside_each_other_count_once() {
    let bench = Bench::new("measure-overlap");
    fs::create_dir_all(bench.at("a/b")).unwrap();
    fs::write(bench.at("a/b/file"), [0; 11]).unwrap();
    let client = bench.client();

    let id = MeasureId(4);
    measure(
        &client,
        id,
        vec![
            bench.at("a/b/file"),
            bench.at("a"),
            bench.at("a/b"),
            bench.at("a"),
            bench.at("a/b/file"),
        ],
    );
    let size = measured(&client, id);

    assert_eq!((size.bytes, size.files, size.folders), (11, 1, 2));
}

#[test]
fn a_path_that_is_not_absolute_is_not_measured() {
    let bench = Bench::new("measure-relative");
    let client = bench.client();

    let id = MeasureId(5);
    measure(&client, id, vec![PathBuf::from("tmp")]);

    assert_eq!(
        measured(&client, id),
        Size {
            unreadable: 1,
            ..Size::default()
        }
    );
}

#[test]
fn only_the_client_that_asked_hears_the_scan() {
    let bench = Bench::new("measure-one-client");
    fs::write(bench.at("file"), [0; 5]).unwrap();
    let (asker, other) = (bench.client(), bench.client());

    let id = MeasureId(9);
    measure(&asker, id, vec![bench.at("file")]);
    measured(&asker, id);

    assert_eq!(
        other.events.recv_timeout(QUIET).err(),
        Some(RecvTimeoutError::Timeout)
    );
}
