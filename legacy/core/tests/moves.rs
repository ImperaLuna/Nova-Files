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
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use nova_files_api::uri::file_uri;
use nova_files_api::{
    Answer, ConflictKind, Event, Failure, JobId, JobTag, Operation, Outcome, Scope,
};
use nova_files_clipboard_api::{FileClipboard, GNOME_COPIED_FILES, Offer};
use nova_files_transfer_api::{self as transfer_api, DropAskId, DropKind};

use answers::{answer, held};
use common::{ANSWER_TIMEOUT, Bench, Client};
use files::{hidden, tree};

const OTHER_DRIVE: &str = "/dev/shm";

fn moving(sources: Vec<PathBuf>, into: &Path) -> Operation {
    Operation::Move {
        sources,
        into: into.to_owned(),
    }
}

fn small_tree(inside: &Path) -> PathBuf {
    let source = inside.join("src");
    fs::create_dir_all(source.join("sub")).unwrap();
    fs::write(source.join("a.txt"), "alpha").unwrap();
    fs::write(source.join("sub/b"), "beta").unwrap();
    symlink("a.txt", source.join("link")).unwrap();
    source
}

fn answer_next(client: &Client, job: JobId, with: Answer) -> ConflictKind {
    let conflict = held(client, job);
    answer(client, job, &conflict, with, Scope::ThisOne);
    conflict.kind
}

fn drop_onto(client: &Client, sources: Vec<PathBuf>, into: &Path) -> (JobId, Operation) {
    let tag = JobTag(9);
    client
        .connection
        .send(transfer_api::Command::Drop {
            tag,
            sources,
            into: into.to_owned(),
        })
        .unwrap();
    match client.next() {
        Event::JobStarted {
            tag: got,
            job,
            operation,
        } if got == tag => (job, operation),
        other => panic!("expected the drop's job first, got {other:?}"),
    }
}

#[test]
fn a_drop_on_one_drive_moves_and_across_drives_copies() {
    let bench = Bench::new("move-drop");
    let near = bench.at("near.txt");
    fs::write(&near, "near").unwrap();
    let into = bench.at("into");
    fs::create_dir(&into).unwrap();
    let client = bench.client();

    let (job, operation) = drop_onto(&client, vec![near.clone()], &into);
    assert_eq!(operation, moving(vec![near.clone()], &into));
    assert_eq!(client.ended(job).0, Outcome::Done);
    assert!(!near.exists());
    assert_eq!(fs::read_to_string(into.join("near.txt")).unwrap(), "near");

    let other = Path::new(OTHER_DRIVE).join(format!("nova-files-drop-{}", std::process::id()));
    fs::remove_dir_all(&other).ok();
    if fs::create_dir(&other).is_err()
        || fs::metadata(&other).unwrap().dev() == fs::metadata(&into).unwrap().dev()
    {
        eprintln!("no other drive at {OTHER_DRIVE} here, so nothing is dropped across drives");
        return;
    }
    let far = other.join("far.txt");
    fs::write(&far, "far").unwrap();
    let (job, operation) = drop_onto(&client, vec![far.clone()], &into);
    assert!(matches!(operation, Operation::Copy { .. }));
    assert_eq!(client.ended(job).0, Outcome::Done);
    assert_eq!(
        fs::read_to_string(&far).unwrap(),
        "far",
        "a copy leaves the source"
    );
    assert_eq!(fs::read_to_string(into.join("far.txt")).unwrap(), "far");
    fs::remove_dir_all(&other).ok();
}

fn drop_would(bench: &Bench, id: u64, sources: Vec<PathBuf>, into: &Path) -> DropKind {
    let id = DropAskId(id);
    let (sink, events) = mpsc::channel();
    let connection = bench.core.connector.connect(sink).unwrap();
    connection
        .send(transfer_api::Command::DropWould {
            id,
            sources,
            into: into.to_owned(),
        })
        .unwrap();
    match events.recv_timeout(ANSWER_TIMEOUT) {
        Ok(transfer_api::Event::DropWould { id: got, kind }) if got == id => kind,
        other => panic!("expected what the drop would do, got {other:?}"),
    }
}

#[test]
fn the_core_says_what_a_drop_would_do_and_does_nothing() {
    let bench = Bench::new("drop-would");
    let near = bench.at("near.txt");
    fs::write(&near, "near").unwrap();
    let into = bench.at("into");
    fs::create_dir(&into).unwrap();

    let kind = drop_would(&bench, 1, vec![near.clone()], &into);
    assert_eq!(kind, DropKind::Move);
    let far = PathBuf::from("/proc/self");
    assert_eq!(
        drop_would(&bench, 2, vec![near.clone(), far], &into),
        DropKind::Copy
    );
    let gone = bench.at("gone");
    assert_eq!(drop_would(&bench, 3, vec![gone], &into), DropKind::Copy);
    assert!(near.exists(), "asking moves nothing");
    assert!(fs::read_dir(&into).unwrap().next().is_none());
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
fn a_file_reached_through_a_moved_link_moves_too() {
    let bench = Bench::new("move-through-link");
    let real = bench.at("real");
    fs::create_dir(&real).unwrap();
    fs::write(real.join("f"), "f").unwrap();
    let alias = bench.at("alias");
    symlink(&real, &alias).unwrap();
    let into = bench.at("into");
    fs::create_dir(&into).unwrap();
    let client = bench.client();

    let (_, outcome, _) = client.run(moving(vec![alias.clone(), alias.join("f")], &into));

    assert_eq!(outcome, Outcome::Done);
    assert_eq!(fs::read_to_string(into.join("f")).unwrap(), "f");
    assert!(into.join("alias").symlink_metadata().unwrap().is_symlink());
    assert!(!real.join("f").exists());
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

#[test]
fn a_cut_from_another_file_manager_pastes_as_a_move() {
    let bench = Bench::new("clipboard-cut");
    fs::create_dir_all(bench.at("from/folder")).unwrap();
    fs::write(bench.at("from/folder/inside"), b"kept").unwrap();
    fs::write(bench.at("from/a b.txt"), b"a").unwrap();
    fs::create_dir(bench.at("into")).unwrap();
    let gnome = format!(
        "cut\n{}\n{}",
        file_uri(&bench.at("from/a b.txt")),
        file_uri(&bench.at("from/folder"))
    );
    let files = FileClipboard::from_offers(&[Offer::new(GNOME_COPIED_FILES, gnome)]).unwrap();

    let (_, outcome, _) = bench
        .client()
        .run(files.paste_into(bench.at("into")).unwrap());

    assert_eq!(outcome, Outcome::Done);
    assert!(!bench.at("from/a b.txt").exists());
    assert!(!bench.at("from/folder").exists());
    assert_eq!(fs::read(bench.at("into/a b.txt")).unwrap(), b"a");
    assert_eq!(fs::read(bench.at("into/folder/inside")).unwrap(), b"kept");
}

#[test]
fn a_folder_swapped_for_a_link_while_the_move_waits_moves_nothing_from_outside() {
    let bench = Bench::new("move-swap");
    let source = bench.at("tree");
    fs::create_dir_all(source.join("sub")).unwrap();
    fs::write(source.join("sub/f"), "planned").unwrap();
    let outside = bench.at("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("f"), "outside").unwrap();
    let into = bench.at("into");
    fs::create_dir_all(into.join("tree/sub")).unwrap();
    let client = bench.client();

    let job = client.start(moving(vec![source.clone()], &into));
    let conflict = held(&client, job);
    fs::remove_dir_all(source.join("sub")).unwrap();
    symlink(&outside, source.join("sub")).unwrap();
    answer(&client, job, &conflict, Answer::Merge, Scope::AllOfThisKind);
    let changed = answer_next(&client, job, Answer::Skip);
    let (outcome, _) = client.ended(job);

    assert_eq!(changed, ConflictKind::SourceChanged);
    assert_eq!(outcome, Outcome::Done);
    assert_eq!(fs::read_to_string(outside.join("f")).unwrap(), "outside");
    assert!(!into.join("tree/sub/f").exists());
    assert!(source.join("sub").is_symlink());
}
