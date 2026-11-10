use std::fs;
use std::path::PathBuf;

use rustix::fs::{CWD, FileType, Mode, mknodat};

use super::*;
use crate::ops::plan::Entry;
use crate::ops::test_folder::TestFolder;

/// Another drive than the scratch folders', where the system has one.
const OTHER_DRIVE: &str = "/dev/shm";

/// Moves `from`, a `kind`, to `target`.
fn move_to(from: PathBuf, kind: Kind, target: &Path) -> Result<Made, Stop> {
    let item = Item {
        from,
        to: target.to_path_buf(),
        kind,
        entry: Entry::default(),
    };
    let making = Making {
        item: &item,
        target,
        index: 0,
        replacing: false,
        how: How::Move,
    };
    make(&mut Running::for_test(), &making)
}

fn failed_at(made: Result<Made, Stop>) -> (PathBuf, Failure) {
    match made {
        Err(Stop::Failed { path, reason }) => (path, reason),
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[test]
fn a_rename_that_fails_on_one_drive_copies_nothing() {
    let folder = TestFolder::new("make-rename-fails");
    // A pipe: a copy would refuse it, so only the rename's error shows.
    let pipe = folder.path().join("pipe");
    mknodat(CWD, &pipe, FileType::Fifo, Mode::RUSR | Mode::WUSR, 0).unwrap();
    let taken = folder.file("taken", "kept");

    let made = move_to(pipe.clone(), Kind::Special, &taken);

    assert_eq!(failed_at(made), (taken.clone(), Failure::NameTaken));
    assert!(pipe.symlink_metadata().is_ok());
    assert_eq!(fs::read_to_string(&taken).unwrap(), "kept");
}

#[test]
fn a_failed_rename_is_at_the_source_only_when_the_source_is_gone() {
    let folder = TestFolder::new("make-rename-where");
    let gone = folder.path().join("gone");
    let file = folder.file("a", "a");
    let free = folder.path().join("b");
    let nowhere = folder.path().join("none").join("b");

    let made = move_to(gone.clone(), Kind::File { size: 0 }, &free);
    assert_eq!(failed_at(made), (gone, Failure::NotFound));
    let made = move_to(file.clone(), Kind::File { size: 1 }, &nowhere);
    assert_eq!(failed_at(made), (nowhere, Failure::NotFound));
    assert!(file.exists());
}

#[test]
fn a_move_to_another_drive_copies_then_removes_the_source() {
    let other = Path::new(OTHER_DRIVE).join(format!("nova-files-make-{}", std::process::id()));
    fs::remove_dir_all(&other).ok();
    if fs::create_dir(&other).is_err() {
        eprintln!("no {OTHER_DRIVE} here, so there is no other drive to move from");
        return;
    }
    let source = other.join("a");
    fs::write(&source, "moved").unwrap();
    let folder = TestFolder::new("make-across");
    let target = folder.path().join("a");

    let made = move_to(source.clone(), Kind::File { size: 5 }, &target);
    fs::remove_dir_all(&other).ok();

    assert!(made.is_ok(), "{made:?}");
    assert!(!source.exists());
    assert_eq!(fs::read_to_string(&target).unwrap(), "moved");
}

#[test]
fn only_the_same_entry_at_the_same_version_is_unchanged() {
    let folder = TestFolder::new("make-unchanged");
    let a = folder.file("a", "a").metadata().unwrap();
    let b = folder.file("b", "a").metadata().unwrap();

    assert!(unchanged(&a, &a));
    assert!(!unchanged(&a, &b));
}

#[test]
fn a_source_that_cant_go_takes_its_copy_along_unless_the_copy_replaced_a_file() {
    let folder = TestFolder::new("make-source-stays");
    let running = Running::for_test();
    running.shared.control.cancel();
    for replacing in [false, true] {
        let source = folder.file("a", "moved");
        let target = folder.file("b", "moved");
        let before = source.symlink_metadata().unwrap();
        let item = Item {
            from: source.clone(),
            to: target.clone(),
            kind: Kind::File { size: 5 },
            entry: Entry::default(),
        };
        let making = Making {
            item: &item,
            target: &target,
            index: 0,
            replacing,
            how: How::Move,
        };

        let copy = target.symlink_metadata().ok();

        let removed = remove_source(&running, &making, &before, copy.as_ref());

        assert!(matches!(removed, Err(Stop::Cancelled)));
        assert!(source.exists());
        assert_eq!(target.exists(), replacing, "replacing: {replacing}");
    }
}

#[test]
fn a_target_saved_over_since_the_copy_stays() {
    let folder = TestFolder::new("make-target-saved");
    let running = Running::for_test();
    running.shared.control.cancel();
    let source = folder.file("a", "moved");
    let before = source.symlink_metadata().unwrap();
    let copy = folder.file("b", "moved").symlink_metadata().unwrap();
    // Another program saves over the copy.
    let saved = folder.file("saved", "newer");
    let target = folder.path().join("b");
    fs::rename(&saved, &target).unwrap();
    let item = Item {
        from: source,
        to: target.clone(),
        kind: Kind::File { size: 5 },
        entry: Entry::default(),
    };
    let making = Making {
        item: &item,
        target: &target,
        index: 0,
        replacing: false,
        how: How::Move,
    };

    assert!(remove_source(&running, &making, &before, Some(&copy)).is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "newer");
    assert!(remove_source(&running, &making, &before, None).is_err());
    assert!(target.exists());
}

#[test]
fn a_folder_that_cant_be_synced_stops_the_step() {
    let folder = TestFolder::new("make-sync-folder");
    let file = folder.file("a", "a");
    assert!(sync_folder_of(&file).is_ok());
    assert!(sync_folder_of(&folder.path().join("gone/a")).is_err());
}
