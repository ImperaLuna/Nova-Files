#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

#[expect(dead_code, reason = "the job helpers; disk space needs only the bench")]
mod common;

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

use nova_files_api::Connection;
use nova_files_drives_api::{Command, Event, MAX_DISK_SPACE_PATHS};

use common::{ANSWER_TIMEOUT, Bench};

fn connect(bench: &Bench) -> (Connection, Receiver<Event>) {
    let (sink, events) = mpsc::channel();
    (bench.core.connector.connect(sink).unwrap(), events)
}

fn ask(connection: &Connection, events: &Receiver<Event>, asked: Vec<PathBuf>) -> Event {
    connection.send(Command::DiskSpace(asked)).unwrap();
    events.recv_timeout(ANSWER_TIMEOUT).unwrap()
}

#[test]
fn a_client_hears_each_paths_space_in_the_order_asked() {
    let bench = Bench::new("disk-space");
    let (connection, events) = connect(&bench);
    let asked = vec![
        bench.at(""),
        bench.at("missing"),
        PathBuf::from("relative"),
        PathBuf::from("/"),
    ];
    let Event::DiskSpace(spaces) = ask(&connection, &events, asked.clone()) else {
        panic!("expected the disk space");
    };
    let paths: Vec<PathBuf> = spaces.iter().map(|answer| answer.path.clone()).collect();
    assert_eq!(paths, asked);
    let here = spaces[0].space.unwrap();
    assert!(here.total > 0);
    assert!(here.free <= here.total);
    assert_eq!(spaces[1].space, None);
    assert_eq!(spaces[2].space, None);
    assert!(spaces[3].space.is_some());
}

#[test]
fn paths_past_the_cap_are_left_out() {
    let bench = Bench::new("disk-space-cap");
    let (connection, events) = connect(&bench);
    let asked = vec![PathBuf::from("/"); MAX_DISK_SPACE_PATHS + 1];
    let Event::DiskSpace(spaces) = ask(&connection, &events, asked) else {
        panic!("expected the disk space");
    };
    assert_eq!(spaces.len(), MAX_DISK_SPACE_PATHS);
}
