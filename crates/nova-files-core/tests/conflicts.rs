//! Conflicts through the real core: a copy holds at whatever stands in an
//! item's way until a client answers, and does what the answer says, and
//! nothing more.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

mod common;
#[path = "common/files.rs"]
mod files;

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use nova_files_api::{Answer, Command, Conflict, ConflictKind, Event, JobId, Outcome, Scope};

use common::{Bench, Client};
use files::{copy, hidden, tree};

/// The conflict job `job` holds at next, skipping its progress.
fn held(client: &Client, job: JobId) -> Conflict {
    loop {
        match client.next() {
            Event::JobConflict { job: at, conflict } if at == job => return conflict,
            Event::JobProgress { .. } => {}
            other => panic!("expected job {job:?} to hold at a conflict, got {other:?}"),
        }
    }
}

/// Answers `conflict`, which job `job` holds at, and waits until every
/// client is told.
fn answer(client: &Client, job: JobId, conflict: &Conflict, answer: Answer, scope: Scope) {
    client.send(Command::AnswerConflict {
        job,
        conflict: conflict.clone(),
        answer,
        scope,
    });
    loop {
        match client.next() {
            Event::ConflictAnswered {
                job: at,
                answer: heard,
            } if at == job => {
                assert_eq!(heard, answer);
                return;
            }
            Event::JobProgress { .. } => {}
            other => panic!("expected the answer to be heard, got {other:?}"),
        }
    }
}

/// Copies `source` into `into`, answers the one conflict it holds at with
/// `with`, and returns that conflict and how the job ended.
fn copy_answering(
    client: &Client,
    source: PathBuf,
    into: &Path,
    with: Answer,
) -> (Conflict, Outcome) {
    let job = client.start(copy(vec![source], into));
    let conflict = held(client, job);
    answer(client, job, &conflict, with, Scope::ThisOne);
    let (outcome, _) = client.ended(job);
    (conflict, outcome)
}

/// A file `a.txt` holding "new", and a folder `into` already holding an
/// `a.txt` with "old".
fn file_over_file(bench: &Bench) -> (PathBuf, PathBuf) {
    let into = bench.at("into");
    fs::create_dir(&into).unwrap();
    fs::write(into.join("a.txt"), "old").unwrap();
    fs::write(bench.at("a.txt"), "new").unwrap();
    (bench.at("a.txt"), into)
}

fn text(path: PathBuf) -> String {
    fs::read_to_string(path).unwrap()
}

#[test]
fn copying_again_asks_and_skip_keeps_the_first_copy() {
    let bench = Bench::new("conflict-skip");
    let into = bench.at("into");
    fs::create_dir(&into).unwrap();
    let source = bench.at("a.txt");
    fs::write(&source, "first").unwrap();
    let client = bench.client();
    let (_, first, _) = client.run(copy(vec![source.clone()], &into));
    fs::write(&source, "second").unwrap();

    let (conflict, outcome) = copy_answering(&client, source.clone(), &into, Answer::Skip);

    assert_eq!(first, Outcome::Done);
    assert_eq!(
        conflict,
        Conflict {
            kind: ConflictKind::FileExists,
            source,
            target: into.join("a.txt"),
            number: 1,
        }
    );
    assert_eq!(outcome, Outcome::Done);
    assert_eq!(text(into.join("a.txt")), "first");
    assert_eq!(hidden(&bench.root), Vec::<PathBuf>::new());
}

#[test]
fn replace_puts_the_new_file_in_and_leaves_no_backup() {
    let bench = Bench::new("conflict-replace");
    let (source, into) = file_over_file(&bench);
    let client = bench.client();

    let (_, outcome) = copy_answering(&client, source, &into, Answer::Replace);

    assert_eq!(outcome, Outcome::Done);
    assert_eq!(text(into.join("a.txt")), "new");
    assert_eq!(hidden(&bench.root), Vec::<PathBuf>::new());
}

#[test]
fn keep_both_numbers_the_new_file() {
    let bench = Bench::new("conflict-both");
    let (source, into) = file_over_file(&bench);
    fs::write(into.join("a (2).txt"), "older").unwrap();
    let client = bench.client();

    let (_, outcome) = copy_answering(&client, source, &into, Answer::KeepBoth);

    assert_eq!(outcome, Outcome::Done);
    assert_eq!(text(into.join("a.txt")), "old");
    assert_eq!(text(into.join("a (2).txt")), "older");
    assert_eq!(text(into.join("a (3).txt")), "new");
}

#[test]
fn replace_on_a_link_replaces_the_link_not_what_it_points_to() {
    let bench = Bench::new("conflict-link");
    let into = bench.at("into");
    fs::create_dir(&into).unwrap();
    fs::write(bench.at("kept"), "kept").unwrap();
    symlink(bench.at("kept"), into.join("a.txt")).unwrap();
    fs::write(bench.at("a.txt"), "new").unwrap();
    let client = bench.client();

    let (_, outcome) = copy_answering(&client, bench.at("a.txt"), &into, Answer::Replace);

    assert_eq!(outcome, Outcome::Done);
    assert!(!into.join("a.txt").is_symlink());
    assert_eq!(text(into.join("a.txt")), "new");
    assert_eq!(text(bench.at("kept")), "kept");
}

#[test]
fn merge_fills_the_folder_there_and_asks_about_each_clash_inside() {
    let bench = Bench::new("conflict-merge");
    let source = bench.at("src");
    fs::create_dir_all(source.join("sub")).unwrap();
    fs::write(source.join("a.txt"), "new").unwrap();
    fs::write(source.join("sub/b"), "b").unwrap();
    let into = bench.at("into");
    fs::create_dir_all(into.join("src")).unwrap();
    fs::write(into.join("src/a.txt"), "old").unwrap();
    fs::write(into.join("src/mine"), "mine").unwrap();
    let client = bench.client();

    let job = client.start(copy(vec![source], &into));
    let folder = held(&client, job);
    answer(&client, job, &folder, Answer::Merge, Scope::ThisOne);
    let file = held(&client, job);
    answer(&client, job, &file, Answer::Skip, Scope::ThisOne);
    let (outcome, _) = client.ended(job);

    assert_eq!(folder.kind, ConflictKind::FolderExists);
    assert_eq!(file.target, into.join("src/a.txt"));
    assert_eq!(outcome, Outcome::Done);
    assert_eq!(
        tree(&into.join("src")),
        vec![
            ("a.txt".into(), "file [111, 108, 100]".to_owned()),
            ("mine".into(), "file [109, 105, 110, 101]".to_owned()),
            ("sub".into(), "folder".to_owned()),
            ("sub/b".into(), "file [98]".to_owned()),
        ]
    );
}

#[test]
fn keep_both_on_a_folder_renames_only_the_top_folder() {
    let bench = Bench::new("conflict-folder-both");
    let source = bench.at("src");
    fs::create_dir_all(source.join("sub")).unwrap();
    fs::write(source.join("sub/b"), "b").unwrap();
    let into = bench.at("into");
    fs::create_dir_all(into.join("src")).unwrap();
    let client = bench.client();

    let (_, outcome) = copy_answering(&client, source, &into, Answer::KeepBoth);

    assert_eq!(outcome, Outcome::Done);
    assert_eq!(tree(&into.join("src")), Vec::new());
    assert_eq!(
        tree(&into.join("src (2)")),
        vec![
            ("sub".into(), "folder".to_owned()),
            ("sub/b".into(), "file [98]".to_owned()),
        ]
    );
}

#[test]
fn skipping_a_folder_skips_all_it_holds_and_still_counts_it_done() {
    let bench = Bench::new("conflict-folder-skip");
    let source = bench.at("src");
    fs::create_dir_all(source.join("sub")).unwrap();
    fs::write(source.join("sub/b"), "b").unwrap();
    let into = bench.at("into");
    fs::create_dir(&into).unwrap();
    fs::write(into.join("src"), "a file").unwrap();
    let client = bench.client();

    let job = client.start(copy(vec![source], &into));
    let conflict = held(&client, job);
    answer(&client, job, &conflict, Answer::Skip, Scope::ThisOne);
    let mut last = None;
    let outcome = loop {
        match client.next() {
            Event::JobProgress { progress, .. } => last = Some(progress),
            Event::JobFinished { outcome, .. } => break outcome,
            other => panic!("unexpected {other:?}"),
        }
    };

    assert_eq!(conflict.kind, ConflictKind::KindMismatch);
    assert_eq!(outcome, Outcome::Done);
    assert_eq!(text(into.join("src")), "a file");
    let last = last.unwrap();
    assert_eq!(
        (last.done_items, last.done_bytes),
        (last.total_items, last.total_bytes)
    );
}

#[test]
fn an_answer_for_all_of_a_kind_is_not_asked_again() {
    let bench = Bench::new("conflict-all");
    let source = bench.at("src");
    fs::create_dir(&source).unwrap();
    let into = bench.at("into");
    fs::create_dir_all(into.join("src")).unwrap();
    for name in ["a", "b", "c"] {
        fs::write(source.join(name), "new").unwrap();
        fs::write(into.join("src").join(name), "old").unwrap();
    }
    let client = bench.client();

    let job = client.start(copy(vec![source], &into));
    let folder = held(&client, job);
    answer(&client, job, &folder, Answer::Merge, Scope::ThisOne);
    let file = held(&client, job);
    answer(&client, job, &file, Answer::Replace, Scope::AllOfThisKind);
    let (outcome, _) = client.ended(job);

    assert_eq!(
        (folder.kind, file.kind),
        (ConflictKind::FolderExists, ConflictKind::FileExists)
    );
    assert_eq!(outcome, Outcome::Done);
    for name in ["a", "b", "c"] {
        assert_eq!(text(into.join("src").join(name)), "new");
    }
}

#[test]
fn an_answer_the_conflict_does_not_take_or_to_another_conflict_is_dropped() {
    let bench = Bench::new("conflict-wrong");
    let (source, into) = file_over_file(&bench);
    let client = bench.client();

    let job = client.start(copy(vec![source], &into));
    let conflict = held(&client, job);
    let mut stale = conflict.clone();
    stale.target = into.join("b.txt");
    for (to, with) in [(&conflict, Answer::Merge), (&stale, Answer::Replace)] {
        client.send(Command::AnswerConflict {
            job,
            conflict: to.clone(),
            answer: with,
            scope: Scope::ThisOne,
        });
    }
    answer(&client, job, &conflict, Answer::Skip, Scope::ThisOne);
    let (outcome, _) = client.ended(job);

    assert_eq!(outcome, Outcome::Done);
    assert_eq!(text(into.join("a.txt")), "old");
}

#[test]
fn a_file_copied_onto_itself_is_kept_under_a_new_name() {
    let bench = Bench::new("conflict-same");
    fs::write(bench.at("a.txt"), "same").unwrap();
    let client = bench.client();

    let (conflict, outcome) =
        copy_answering(&client, bench.at("a.txt"), &bench.root, Answer::KeepBoth);

    assert_eq!(conflict.kind, ConflictKind::SameFile);
    assert_eq!(outcome, Outcome::Done);
    assert_eq!(text(bench.at("a.txt")), "same");
    assert_eq!(text(bench.at("a (2).txt")), "same");
}

#[test]
fn a_job_held_at_a_conflict_can_be_cancelled_and_is_replayed_to_watchers() {
    let bench = Bench::new("conflict-cancel");
    let (source, into) = file_over_file(&bench);
    let client = bench.client();

    let job = client.start(copy(vec![source], &into));
    let conflict = held(&client, job);
    let watcher = bench.client();
    watcher.send(Command::WatchJobs);
    let Event::JobRunning {
        job: heard,
        conflict: replayed,
        ..
    } = watcher.next()
    else {
        panic!("the held job comes first");
    };
    client.send(Command::CancelJob(job));
    let (outcome, _) = client.ended(job);

    assert_eq!((heard, replayed), (job, Some(conflict)));
    assert_eq!(outcome, Outcome::Cancelled);
    assert_eq!(text(into.join("a.txt")), "old");
}

#[test]
fn replace_looks_again_at_what_has_the_name_now() {
    let bench = Bench::new("conflict-changed");
    let (source, into) = file_over_file(&bench);
    let client = bench.client();

    let job = client.start(copy(vec![source], &into));
    let file = held(&client, job);
    // While the question is open, the file there becomes a folder.
    fs::remove_file(into.join("a.txt")).unwrap();
    fs::create_dir(into.join("a.txt")).unwrap();
    answer(&client, job, &file, Answer::Replace, Scope::ThisOne);
    let folder = held(&client, job);
    // And then goes.
    fs::remove_dir(into.join("a.txt")).unwrap();
    answer(&client, job, &folder, Answer::KeepBoth, Scope::ThisOne);
    let (outcome, _) = client.ended(job);

    assert_eq!(folder.kind, ConflictKind::KindMismatch);
    assert_eq!(outcome, Outcome::Done);
    assert_eq!(text(into.join("a (2).txt")), "new");
}

#[test]
fn an_answer_for_this_one_is_asked_again_and_skip_skips_a_gone_source() {
    let bench = Bench::new("conflict-this-one");
    let into = bench.at("into");
    fs::create_dir(&into).unwrap();
    for name in ["a", "b", "c"] {
        fs::write(bench.at(name), "new").unwrap();
    }
    for name in ["a", "b"] {
        fs::write(into.join(name), "old").unwrap();
    }
    let client = bench.client();
    let sources = ["a", "b", "c"].map(|name| bench.at(name)).to_vec();

    let job = client.start(copy(sources, &into));
    let first = held(&client, job);
    answer(&client, job, &first, Answer::Skip, Scope::ThisOne);
    let second = held(&client, job);
    fs::remove_file(bench.at("c")).unwrap();
    answer(&client, job, &second, Answer::Replace, Scope::ThisOne);
    let gone = held(&client, job);
    answer(&client, job, &gone, Answer::Skip, Scope::ThisOne);
    let (outcome, _) = client.ended(job);

    assert_eq!([first.number, second.number, gone.number], [1, 2, 3]);
    assert_eq!(
        [first.kind, second.kind, gone.kind],
        [
            ConflictKind::FileExists,
            ConflictKind::FileExists,
            ConflictKind::SourceGone
        ]
    );
    assert_eq!(outcome, Outcome::Done);
    assert_eq!(text(into.join("a")), "old");
    assert_eq!(text(into.join("b")), "new");
    assert!(!into.join("c").exists());
}

#[test]
fn two_folders_of_the_same_name_are_each_asked_about() {
    let bench = Bench::new("conflict-same-name");
    let into = bench.at("into");
    fs::create_dir_all(into.join("photos")).unwrap();
    let sources = ["a", "b"].map(|from| {
        let photos = bench.at(from).join("photos");
        fs::create_dir_all(&photos).unwrap();
        fs::write(photos.join(from), from).unwrap();
        photos
    });
    let client = bench.client();

    let job = client.start(copy(sources.to_vec(), &into));
    let first = held(&client, job);
    answer(&client, job, &first, Answer::Skip, Scope::ThisOne);
    let second = held(&client, job);
    answer(&client, job, &second, Answer::Merge, Scope::ThisOne);
    let (outcome, _) = client.ended(job);

    assert_eq!([first.source, second.source], sources);
    assert_eq!(outcome, Outcome::Done);
    assert!(!into.join("photos/a").exists());
    assert_eq!(text(into.join("photos/b")), "b");
}

#[test]
fn a_source_inside_another_gets_its_own_answers() {
    let bench = Bench::new("conflict-nested-source");
    let into = bench.at("into");
    fs::create_dir_all(into.join("photos")).unwrap();
    let photos = bench.at("photos");
    let child = photos.join("child");
    fs::create_dir_all(&child).unwrap();
    fs::write(child.join("c"), "c").unwrap();
    let client = bench.client();

    let job = client.start(copy(vec![photos, child], &into));
    let folder = held(&client, job);
    answer(&client, job, &folder, Answer::Skip, Scope::ThisOne);
    let (outcome, _) = client.ended(job);

    assert_eq!(folder.kind, ConflictKind::FolderExists);
    assert_eq!(outcome, Outcome::Done);
    assert!(!into.join("photos/child").exists());
    assert_eq!(text(into.join("child/c")), "c");
}
