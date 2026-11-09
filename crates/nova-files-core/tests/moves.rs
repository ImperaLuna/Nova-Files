//! Moving through the real core: on one drive by renaming, across drives
//! by copying and then removing each source item, and through the same
//! conflicts a copy asks.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

#[path = "common/answers.rs"]
mod answers;
mod common;
#[path = "common/files.rs"]
mod files;

use std::fs::{self, File};
use std::os::unix::fs::{MetadataExt as _, symlink};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use nova_files_api::{Answer, ConflictKind, Event, Failure, JobId, Operation, Outcome, Scope};

use answers::{answer, held};
use common::{Bench, Client};
use files::{hidden, tree};

/// Another drive than the scratch folders', where the system has one.
const OTHER_DRIVE: &str = "/dev/shm";

fn moving(sources: Vec<PathBuf>, into: &Path) -> Operation {
    Operation::Move {
        sources,
        into: into.to_owned(),
    }
}

/// A folder `src` in `inside`, holding a file, a nested file and a link.
fn small_tree(inside: &Path) -> PathBuf {
    let source = inside.join("src");
    fs::create_dir_all(source.join("sub")).unwrap();
    fs::write(source.join("a.txt"), "alpha").unwrap();
    fs::write(source.join("sub/b"), "beta").unwrap();
    symlink("a.txt", source.join("link")).unwrap();
    source
}

/// Answers the next conflict of job `job` with `with` and returns its
/// kind.
fn answer_next(client: &Client, job: JobId, with: Answer) -> ConflictKind {
    let conflict = held(client, job);
    answer(client, job, &conflict, with, Scope::ThisOne);
    conflict.kind
}

#[test]
fn a_move_on_one_drive_renames_the_tree_whole() {
    let bench = Bench::new("move-rename");
    let source = small_tree(&bench.root);
    let before = tree(&source);
    let inode = fs::metadata(&source).unwrap().ino();
    let into = bench.at("into");
    fs::create_dir(&into).unwrap();
    let client = bench.client();

    let (_, outcome, changed) = client.run(moving(vec![source.clone()], &into));

    assert_eq!(outcome, Outcome::Done);
    assert_eq!(changed, vec![into.clone(), bench.root.clone()]);
    assert!(!source.exists());
    assert_eq!(tree(&into.join("src")), before);
    assert_eq!(fs::metadata(into.join("src")).unwrap().ino(), inode);
}

#[test]
fn a_move_across_drives_copies_then_removes_each_source_item() {
    let bench = Bench::new("move-across");
    let other = Path::new(OTHER_DRIVE).join(format!("nova-files-move-{}", std::process::id()));
    fs::remove_dir_all(&other).ok();
    if fs::create_dir(&other).is_err() {
        eprintln!("no {OTHER_DRIVE} here, so this moves on one drive");
    }
    let from = if other.is_dir() {
        other.clone()
    } else {
        bench.at("from")
    };
    fs::create_dir_all(&from).unwrap();
    let source = small_tree(&from);
    let before = tree(&source);
    // Renamed whole, beside the tree that can't be.
    let near = bench.at("near.txt");
    fs::write(&near, "near").unwrap();
    let into = bench.at("into");
    fs::create_dir(&into).unwrap();
    let client = bench.client();

    let job = client.start(moving(vec![source.clone(), near], &into));
    let mut progress = None;
    let outcome = loop {
        match client.next() {
            Event::JobProgress { progress: now, .. } => progress = Some(now),
            Event::JobFinished {
                job: ended,
                outcome,
                ..
            } if ended == job => break outcome,
            other => panic!("expected the move to run to its end, got {other:?}"),
        }
    };

    assert_eq!(outcome, Outcome::Done);
    assert!(!source.exists(), "every source item goes, folders last");
    assert_eq!(tree(&into.join("src")), before);
    assert_eq!(fs::read_to_string(into.join("near.txt")).unwrap(), "near");
    // The tree's five items and the file renamed whole, each once.
    let progress = progress.unwrap();
    assert_eq!((progress.done_items, progress.total_items), (6, 6));
    assert_eq!(hidden(&bench.root), Vec::<PathBuf>::new());
    fs::remove_dir_all(&other).ok();
}

#[test]
fn a_merged_move_keeps_the_source_folder_only_while_it_holds_something() {
    let bench = Bench::new("move-merge");
    let source = small_tree(&bench.root);
    let into = bench.at("into");
    fs::create_dir_all(into.join("src")).unwrap();
    fs::write(into.join("src/a.txt"), "old").unwrap();
    let client = bench.client();

    let job = client.start(moving(vec![source.clone()], &into));
    let folder = answer_next(&client, job, Answer::Merge);
    let file = answer_next(&client, job, Answer::Skip);
    let (outcome, _) = client.ended(job);

    assert_eq!(
        (folder, file),
        (ConflictKind::FolderExists, ConflictKind::FileExists)
    );
    assert_eq!(outcome, Outcome::Done);
    assert_eq!(fs::read_to_string(into.join("src/a.txt")).unwrap(), "old");
    assert_eq!(fs::read_to_string(into.join("src/sub/b")).unwrap(), "beta");
    assert_eq!(
        tree(&source),
        vec![("a.txt".into(), "file [97, 108, 112, 104, 97]".to_owned())],
        "only what was skipped stays"
    );
}

#[test]
fn a_move_that_replaces_leaves_one_file_and_no_backup() {
    let bench = Bench::new("move-replace");
    let into = bench.at("into");
    fs::create_dir(&into).unwrap();
    fs::write(into.join("a.txt"), "old").unwrap();
    fs::write(bench.at("a.txt"), "new").unwrap();
    let client = bench.client();

    let job = client.start(moving(vec![bench.at("a.txt")], &into));
    let kind = answer_next(&client, job, Answer::Replace);
    let (outcome, _) = client.ended(job);

    assert_eq!((kind, outcome), (ConflictKind::FileExists, Outcome::Done));
    assert_eq!(fs::read_to_string(into.join("a.txt")).unwrap(), "new");
    assert!(!bench.at("a.txt").exists());
    assert_eq!(hidden(&bench.root), Vec::<PathBuf>::new());
}

#[test]
fn a_file_moved_into_its_own_folder_is_the_same_file() {
    let bench = Bench::new("move-same");
    fs::write(bench.at("a.txt"), "same").unwrap();
    let client = bench.client();

    let job = client.start(moving(vec![bench.at("a.txt")], &bench.root));
    let kind = answer_next(&client, job, Answer::Skip);
    let (outcome, changed) = client.ended(job);

    assert_eq!((kind, outcome), (ConflictKind::SameFile, Outcome::Done));
    assert_eq!(changed, Vec::<PathBuf>::new());
    assert_eq!(fs::read_to_string(bench.at("a.txt")).unwrap(), "same");
}

#[test]
fn a_folder_is_not_moved_into_itself() {
    let bench = Bench::new("move-itself");
    let source = small_tree(&bench.root);
    let before = tree(&source);
    let client = bench.client();

    let (_, outcome, _) = client.run(moving(vec![source.clone()], &source.join("sub")));

    assert_eq!(
        outcome,
        Outcome::Failed {
            path: source.clone(),
            reason: Failure::IntoItself
        }
    );
    assert_eq!(tree(&source), before);
}

#[test]
fn a_source_inside_another_moves_with_it() {
    let bench = Bench::new("move-nested");
    let source = small_tree(&bench.root);
    let before = tree(&source);
    let into = bench.at("into");
    fs::create_dir(&into).unwrap();
    let client = bench.client();

    let (_, outcome, _) = client.run(moving(vec![source.join("sub"), source.clone()], &into));

    assert_eq!(outcome, Outcome::Done);
    assert_eq!(tree(&into.join("src")), before);
    assert!(!into.join("sub").exists());
}

#[test]
fn a_bad_source_moves_nothing() {
    let bench = Bench::new("move-bad");
    let source = small_tree(&bench.root);
    let into = bench.at("into");
    fs::create_dir(&into).unwrap();
    let client = bench.client();

    let (_, outcome, changed) = client.run(moving(vec![source.clone(), bench.at("gone")], &into));

    assert_eq!(
        outcome,
        Outcome::Failed {
            path: bench.at("gone"),
            reason: Failure::NotFound
        }
    );
    assert_eq!(changed, Vec::<PathBuf>::new());
    assert!(source.exists());
    assert_eq!(tree(&into), Vec::new());
}

/// When a folder was last changed, in a test that keeps it.
const LONG_AGO: Duration = Duration::from_secs(1_000_000_000);

#[test]
fn a_folder_moved_across_drives_keeps_its_times() {
    let bench = Bench::new("move-across-times");
    let other = Path::new(OTHER_DRIVE).join(format!("nova-files-times-{}", std::process::id()));
    fs::remove_dir_all(&other).ok();
    if fs::create_dir(&other).is_err() {
        eprintln!("no {OTHER_DRIVE} here, so nothing moves across drives");
        return;
    }
    let source = small_tree(&other);
    let then = SystemTime::UNIX_EPOCH + LONG_AGO;
    File::open(&source).unwrap().set_modified(then).unwrap();
    let into = bench.at("into");
    fs::create_dir(&into).unwrap();
    let client = bench.client();

    let (_, outcome, _) = client.run(moving(vec![source], &into));
    fs::remove_dir_all(&other).ok();

    assert_eq!(outcome, Outcome::Done);
    let moved = fs::metadata(into.join("src")).unwrap();
    assert_eq!(moved.modified().unwrap(), then);
}
