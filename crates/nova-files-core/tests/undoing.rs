//! Undoing jobs through the real core: each client hears which jobs can
//! be undone, an undo takes back what the job did and nothing that
//! changed since, and runs once.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

#[path = "common/answers.rs"]
mod answers;
mod common;

use std::fs;
use std::path::{Path, PathBuf};

use nova_files_api::{Answer, Command, Event, Failure, JobId, Operation, Outcome, Scope};

use common::{Bench, Client};

/// Runs `operation` to its end, hearing that it can be undone before
/// that it ended.
fn undoable(client: &Client, operation: Operation) -> JobId {
    let job = client.start(operation.clone());
    loop {
        match client.any_next() {
            Event::JobProgress { .. } => {}
            heard => {
                assert_eq!(heard, Event::Undoable { job, operation });
                break;
            }
        }
    }
    assert_eq!(client.ended(job).0, Outcome::Done);
    job
}

/// Undoes job `job`, and returns how that went.
fn undo(client: &Client, job: JobId) -> Outcome {
    let undo = client.start(Operation::Undo { job });
    assert_eq!(client.any_next(), Event::UndoGone(job));
    client.ended(undo).0
}

fn home_trash_files(root: &Path) -> PathBuf {
    root.join("home/.local/share/Trash/files")
}

#[test]
fn undoing_a_copy_sends_the_copies_to_the_trash() {
    let bench = Bench::new("undo-copy");
    fs::write(bench.at("a"), "a").unwrap();
    fs::create_dir(bench.at("into")).unwrap();
    let client = bench.client();
    let job = undoable(
        &client,
        Operation::Copy {
            sources: vec![bench.at("a")],
            into: bench.at("into"),
        },
    );

    assert_eq!(undo(&client, job), Outcome::Done);

    assert!(!bench.at("into/a").exists());
    assert!(bench.at("a").exists());
    assert_eq!(
        fs::read_to_string(home_trash_files(&bench.root).join("a")).unwrap(),
        "a"
    );
}

#[test]
fn undoing_a_move_puts_entries_back_under_their_old_names() {
    let bench = Bench::new("undo-move");
    fs::create_dir_all(bench.at("dir/sub")).unwrap();
    fs::write(bench.at("dir/sub/f"), "f").unwrap();
    fs::create_dir(bench.at("into")).unwrap();
    let client = bench.client();
    let job = undoable(
        &client,
        Operation::Move {
            sources: vec![bench.at("dir")],
            into: bench.at("into"),
        },
    );

    assert_eq!(undo(&client, job), Outcome::Done);

    assert_eq!(fs::read_to_string(bench.at("dir/sub/f")).unwrap(), "f");
    assert!(!bench.at("into/dir").exists());
}

#[test]
fn undoing_a_merged_move_takes_back_only_what_went_in() {
    let bench = Bench::new("undo-merge");
    fs::create_dir_all(bench.at("dir/sub")).unwrap();
    fs::write(bench.at("dir/f"), "f").unwrap();
    fs::write(bench.at("dir/sub/g"), "g").unwrap();
    fs::create_dir_all(bench.at("into/dir")).unwrap();
    fs::write(bench.at("into/dir/kept"), "theirs").unwrap();
    let client = bench.client();
    let job = client.start(Operation::Move {
        sources: vec![bench.at("dir")],
        into: bench.at("into"),
    });
    let folder = answers::held(&client, job);
    answers::answer(&client, job, &folder, Answer::Merge, Scope::ThisOne);
    assert_eq!(client.ended(job).0, Outcome::Done);
    assert!(!bench.at("dir").exists(), "emptied, so gone");

    assert_eq!(undo(&client, job), Outcome::Done);

    assert_eq!(fs::read_to_string(bench.at("dir/f")).unwrap(), "f");
    assert_eq!(fs::read_to_string(bench.at("dir/sub/g")).unwrap(), "g");
    let left: Vec<_> = fs::read_dir(bench.at("into/dir"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(left, ["kept"]);
}

#[test]
fn undoing_a_trash_restores_and_undoing_a_restore_trashes_again() {
    let bench = Bench::new("undo-trash");
    fs::write(bench.at("a"), "a").unwrap();
    let client = bench.client();
    let trashed = undoable(
        &client,
        Operation::Trash {
            paths: vec![bench.at("a")],
        },
    );

    assert_eq!(undo(&client, trashed), Outcome::Done);

    assert_eq!(fs::read_to_string(bench.at("a")).unwrap(), "a");
    let item = home_trash_files(&bench.root)
        .canonicalize()
        .unwrap()
        .join("a");
    assert!(!item.exists());
    // The undo itself is a restore, which can't be undone; one asked for
    // can.
    undoable(
        &client,
        Operation::Trash {
            paths: vec![bench.at("a")],
        },
    );
    let restored = undoable(
        &client,
        Operation::Restore {
            items: vec![item.clone()],
        },
    );
    assert!(bench.at("a").exists());

    assert_eq!(undo(&client, restored), Outcome::Done);

    assert!(!bench.at("a").exists());
    assert_eq!(fs::read_to_string(&item).unwrap(), "a");
}

#[test]
fn undoing_a_rename_and_a_new_folder() {
    let bench = Bench::new("undo-one-step");
    fs::write(bench.at("old"), "x").unwrap();
    let client = bench.client();
    let renamed = undoable(
        &client,
        Operation::Rename {
            path: bench.at("old"),
            name: "new".into(),
        },
    );
    let made = undoable(
        &client,
        Operation::NewFolder {
            inside: bench.root.clone(),
            name: "made".into(),
        },
    );

    assert_eq!(undo(&client, renamed), Outcome::Done);
    assert_eq!(undo(&client, made), Outcome::Done);

    assert!(bench.at("old").exists() && !bench.at("new").exists());
    assert!(!bench.at("made").exists());
}

#[test]
fn what_changed_since_is_left_alone() {
    let bench = Bench::new("undo-changed");
    fs::write(bench.at("old"), "x").unwrap();
    let client = bench.client();
    let renamed = undoable(
        &client,
        Operation::Rename {
            path: bench.at("old"),
            name: "new".into(),
        },
    );
    // Replaced by another file under the same name.
    fs::write(bench.at("other"), "someone else's").unwrap();
    fs::rename(bench.at("other"), bench.at("new")).unwrap();

    let outcome = undo(&client, renamed);

    assert!(matches!(
        outcome,
        Outcome::Failed {
            reason: Failure::NotFound,
            ..
        }
    ));
    assert_eq!(
        fs::read_to_string(bench.at("new")).unwrap(),
        "someone else's"
    );
}

/// Whether `outcome` says there was nothing left to take back.
const fn found_nothing(outcome: &Outcome) -> bool {
    matches!(
        outcome,
        Outcome::Failed {
            reason: Failure::NotFound,
            ..
        }
    )
}

#[test]
fn an_undo_with_nothing_left_to_take_back_finds_nothing() {
    let bench = Bench::new("undo-nothing-left");
    fs::write(bench.at("a"), "a").unwrap();
    fs::write(bench.at("b"), "b").unwrap();
    fs::create_dir(bench.at("into")).unwrap();
    let client = bench.client();
    let copied = undoable(
        &client,
        Operation::Copy {
            sources: vec![bench.at("a")],
            into: bench.at("into"),
        },
    );
    let trashed = undoable(
        &client,
        Operation::Trash {
            paths: vec![bench.at("b")],
        },
    );
    let made = undoable(
        &client,
        Operation::NewFolder {
            inside: bench.root.clone(),
            name: "made".into(),
        },
    );
    // The copy replaced by another file, the trashed entry gone from the
    // trash, and the new folder replaced by another, made while it was
    // still there.
    fs::write(bench.at("other"), "someone else's").unwrap();
    fs::rename(bench.at("other"), bench.at("into/a")).unwrap();
    fs::remove_file(home_trash_files(&bench.root).join("b")).unwrap();
    fs::create_dir(bench.at("other-folder")).unwrap();
    fs::remove_dir(bench.at("made")).unwrap();
    fs::rename(bench.at("other-folder"), bench.at("made")).unwrap();

    assert!(found_nothing(&undo(&client, copied)));
    assert!(found_nothing(&undo(&client, trashed)));
    assert!(found_nothing(&undo(&client, made)));

    assert_eq!(
        fs::read_to_string(bench.at("into/a")).unwrap(),
        "someone else's"
    );
    assert!(!bench.at("b").exists());
    assert!(bench.at("made").is_dir());
}

#[test]
fn an_undo_runs_once_and_a_watcher_hears_what_can_be_undone() {
    let bench = Bench::new("undo-once");
    let client = bench.client();
    let job = undoable(
        &client,
        Operation::NewFolder {
            inside: bench.root.clone(),
            name: "made".into(),
        },
    );
    let watcher = bench.client();

    watcher.send(Command::WatchJobs);

    assert!(matches!(
        watcher.any_next(),
        Event::Undoable { job: heard, .. } if heard == job
    ));
    let first = client.start(Operation::Undo { job });
    assert_eq!(client.any_next(), Event::UndoGone(job));
    // A done undo is spent: never offered again before it ends.
    loop {
        match client.any_next() {
            Event::JobProgress { .. } => {}
            heard => {
                assert!(
                    matches!(heard, Event::JobFinished { job: ended, outcome: Outcome::Done, .. } if ended == first),
                    "expected the undo to end, got {heard:?}"
                );
                break;
            }
        }
    }
    assert_eq!(watcher.any_next(), Event::UndoGone(job));
    let again = client.start(Operation::Undo { job });
    assert!(matches!(
        client.ended(again).0,
        Outcome::Failed {
            reason: Failure::NotFound,
            ..
        }
    ));
}

#[test]
fn a_delete_can_not_be_undone() {
    let bench = Bench::new("undo-delete");
    fs::write(bench.at("a"), "a").unwrap();
    let client = bench.client();
    let (_, outcome, _) = client.run(Operation::Delete {
        paths: vec![bench.at("a")],
    });
    assert_eq!(outcome, Outcome::Done);

    client.send(Command::Ping);

    assert_eq!(client.any_next(), Event::Pong, "nothing to undo was heard");
}
