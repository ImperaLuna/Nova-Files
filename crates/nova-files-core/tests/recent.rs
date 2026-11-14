//! Recent folders through the real core: a client says which folders a
//! person opened and hears them back newest first, with a peek inside,
//! leaving out folders it skips or that are gone; the next run still has
//! them.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

#[expect(
    dead_code,
    reason = "the job helpers; recent folders need only the bench"
)]
mod common;

use std::fs;
use std::path::PathBuf;

use nova_files_api::{Command, EntryKind, Event, PEEK_ENTRIES, RecentFolder};

use common::{Bench, Client};

fn recent(client: &Client, count: u32, skip: Vec<PathBuf>) -> Vec<RecentFolder> {
    client.send(Command::RecentFolders { count, skip });
    match client.next() {
        Event::RecentFolders(folders) => folders,
        other => panic!("expected the recent folders, got {other:?}"),
    }
}

fn paths(folders: &[RecentFolder]) -> Vec<PathBuf> {
    folders.iter().map(|folder| folder.path.clone()).collect()
}

#[test]
fn folders_come_back_newest_first_with_a_peek_inside() {
    let bench = Bench::new("recent-peek");
    let photos = bench.at("Photos");
    let notes = bench.at("Notes");
    let skipped = bench.at("Skipped");
    for folder in [&photos, &notes, &skipped] {
        fs::create_dir(folder).unwrap();
    }
    fs::create_dir(photos.join("Trips")).unwrap();
    fs::write(photos.join("Trips/a.jpg"), b"").unwrap();
    for name in ["b.png", "c.png", "d.png", "e.png", "f.png"] {
        fs::write(photos.join(name), b"12345").unwrap();
    }
    let client = bench.client();
    for folder in [&notes, &photos, &skipped, &bench.at("gone")] {
        client.send(Command::FolderUsed(folder.clone()));
    }
    let folders = recent(&client, 5, vec![skipped.clone()]);
    assert_eq!(paths(&folders), vec![photos, notes]);
    let photos_seen = &folders[0];
    assert_eq!(photos_seen.entries, 6);
    assert!(photos_seen.modified.is_some());
    assert_eq!(photos_seen.peek.len(), PEEK_ENTRIES);
    let trips = &photos_seen.peek[0];
    assert_eq!(trips.name, "Trips");
    assert_eq!(trips.kind, EntryKind::Folder);
    assert_eq!(trips.amount, Some(1));
    let first_file = &photos_seen.peek[1];
    assert_eq!(first_file.name, "b.png");
    assert_eq!(first_file.kind, EntryKind::File);
    assert_eq!(first_file.amount, Some(5));
    assert_eq!(folders[1].peek, Vec::new());
    assert_eq!(paths(&recent(&client, 1, Vec::new())), vec![skipped]);
}

#[test]
fn the_next_run_remembers_the_folders() {
    let bench = Bench::new("recent-kept");
    let kept = bench.at("Kept");
    fs::create_dir(&kept).unwrap();
    {
        let client = bench.client();
        client.send(Command::FolderUsed(kept.clone()));
        assert_eq!(paths(&recent(&client, 5, Vec::new())), vec![kept.clone()]);
    }
    let state = bench.at("home/.local/state");
    let file = state.join("nova-files/recent-folders");
    let wait = std::time::Instant::now();
    while !fs::read(&file).is_ok_and(|bytes| bytes.ends_with(b"Kept\0")) {
        assert!(
            wait.elapsed() < std::time::Duration::from_secs(10),
            "never saved"
        );
        std::thread::yield_now();
    }
    // A second core with the same home reads what the first one saved.
    let home = nova_files_core::Home {
        folder: bench.at("home"),
        config: bench.at("home/.config"),
        state,
        data: bench.at("home/.local/share"),
        drive_trashes: false,
    };
    let again =
        nova_files_core::spawn_with(nova_files_core::IDLE_EXIT, Some(home), None, None).unwrap();
    let (sink, events) = std::sync::mpsc::channel();
    let client = Client {
        connection: again.connector.connect(sink).unwrap(),
        events,
    };
    assert_eq!(paths(&recent(&client, 5, Vec::new())), vec![kept]);
}
