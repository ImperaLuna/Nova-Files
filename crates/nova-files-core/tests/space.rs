//! Space scans through the real core, against real temp folders: each
//! entry's share of a folder, folders that can't be listed, and a space
//! scan running beside a size scan.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

#[expect(dead_code, reason = "the job helpers; scans need only the bench")]
mod common;

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;

use nova_files_api::{Command, EntryKind, Event, MeasureId, ScanState, Size, Unreadable};

use common::{Bench, Client};

/// Each entry's last reported kind and totals, once scan `id` is done.
fn shares(client: &Client, id: MeasureId) -> HashMap<OsString, (EntryKind, Size)> {
    let mut latest = HashMap::new();
    loop {
        match client.next() {
            Event::SpaceMeasured {
                id: got,
                shares,
                state,
            } if got == id => {
                for share in shares {
                    latest.insert(share.name, (share.kind, share.size));
                }
                match state {
                    ScanState::Counting => {}
                    ScanState::Done => return latest,
                    ScanState::CutShort => panic!("scan {id:?} was cut short"),
                }
            }
            other => panic!("expected scan {id:?}'s shares, got {other:?}"),
        }
    }
}

fn space(client: &Client, id: MeasureId, folder: PathBuf) {
    client.send(Command::Space { id, folder });
}

#[test]
fn each_entry_gets_its_own_totals() {
    let bench = Bench::new("space-entries");
    fs::create_dir_all(bench.at("top/photos/2026")).unwrap();
    fs::write(bench.at("top/photos/2026/a"), [0; 1000]).unwrap();
    fs::write(bench.at("top/photos/b"), [0; 500]).unwrap();
    fs::write(bench.at("top/notes"), [0; 30]).unwrap();
    fs::create_dir(bench.at("top/empty")).unwrap();
    symlink(bench.at("top/photos"), bench.at("top/link")).unwrap();
    let client = bench.client();

    let id = MeasureId(1);
    space(&client, id, bench.at("top"));
    let shares = shares(&client, id);

    let photos = shares[&OsString::from("photos")];
    assert_eq!(photos.0, EntryKind::Folder);
    assert_eq!(
        (photos.1.bytes, photos.1.files, photos.1.folders),
        (1500, 2, 2)
    );
    let notes = shares[&OsString::from("notes")];
    assert_eq!(
        (notes.0, notes.1.bytes, notes.1.files),
        (EntryKind::File, 30, 1)
    );
    let empty = shares[&OsString::from("empty")].1;
    assert_eq!((empty.bytes, empty.files, empty.folders), (0, 0, 1));
    // The link counts as itself, never what it points to.
    let link = shares[&OsString::from("link")].1;
    assert_eq!(link.files, 1);
    assert!(link.bytes < 1500);
    assert_eq!(shares.len(), 4);
}

#[test]
fn an_empty_folder_has_no_shares() {
    let bench = Bench::new("space-empty");
    fs::create_dir(bench.at("empty")).unwrap();
    let client = bench.client();

    let id = MeasureId(2);
    space(&client, id, bench.at("empty"));

    assert!(shares(&client, id).is_empty());
}

#[test]
fn a_folder_that_cannot_be_listed_says_why() {
    let bench = Bench::new("space-unreadable");
    fs::write(bench.at("file"), [0; 3]).unwrap();
    let client = bench.client();

    let (missing, file, relative) = (MeasureId(3), MeasureId(4), MeasureId(5));
    space(&client, missing, bench.at("gone"));
    let answer = client.next();
    space(&client, file, bench.at("file"));
    let not_a_folder = client.next();
    space(&client, relative, PathBuf::from("tmp"));
    let not_absolute = client.next();

    assert_eq!(
        answer,
        Event::SpaceUnreadable {
            id: missing,
            reason: Unreadable::Missing
        }
    );
    assert_eq!(
        not_a_folder,
        Event::SpaceUnreadable {
            id: file,
            reason: Unreadable::NotAFolder
        }
    );
    assert_eq!(
        not_absolute,
        Event::SpaceUnreadable {
            id: relative,
            reason: Unreadable::Failed
        }
    );
}

#[test]
fn a_space_scan_runs_beside_a_size_scan() {
    let bench = Bench::new("space-beside");
    fs::create_dir(bench.at("top")).unwrap();
    fs::write(bench.at("top/file"), [0; 9]).unwrap();
    let client = bench.client();

    let (size_id, space_id) = (MeasureId(6), MeasureId(7));
    client.send(Command::Measure {
        id: size_id,
        paths: vec![bench.at("top")],
    });
    space(&client, space_id, bench.at("top"));

    let (mut size_done, mut space_done) = (false, false);
    while !(size_done && space_done) {
        match client.next() {
            Event::SizeMeasured { id, size, state } if id == size_id => {
                if state == ScanState::Done {
                    assert_eq!(size.bytes, 9);
                    size_done = true;
                }
            }
            Event::SpaceMeasured { id, state, .. } if id == space_id => {
                space_done = state == ScanState::Done;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}

#[test]
fn every_entry_is_named_even_one_that_adds_nothing() {
    let bench = Bench::new("space-hard-links");
    fs::create_dir(bench.at("top")).unwrap();
    fs::write(bench.at("top/a"), [0; 50]).unwrap();
    fs::hard_link(bench.at("top/a"), bench.at("top/b")).unwrap();
    let client = bench.client();

    let id = MeasureId(8);
    space(&client, id, bench.at("top"));
    let shares = shares(&client, id);

    let files: u64 = shares.values().map(|(_, size)| size.files).sum();
    assert_eq!(shares.len(), 2);
    assert_eq!(files, 1);
}

/// `/dev` has `/dev/shm` mounted inside it on any Linux this runs on;
/// skipped where it doesn't, or where it can't be written.
#[test]
fn a_drive_mounted_on_an_entry_is_not_entered() {
    let shm = PathBuf::from("/dev/shm");
    let big = shm.join(format!("nova-files-space-mount-{}", std::process::id()));
    let Ok(file) = fs::File::create(&big) else {
        return;
    };
    // Sparse: takes no space, but its length counts if it is reached.
    let length = 1 << 50;
    file.set_len(length).unwrap();
    let bench = Bench::new("space-mount");
    let client = bench.client();

    let id = MeasureId(9);
    space(&client, id, PathBuf::from("/dev"));
    let shares = shares(&client, id);

    fs::remove_file(&big).ok();
    if let Some((_, size)) = shares.get(&OsString::from("shm")) {
        assert!(size.bytes < length, "{size:?}");
    }
}
