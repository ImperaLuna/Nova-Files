use crate::answers;
use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use nova_files_api::{Answer, Command, Event, Failure, JobId, Operation, Outcome, Scope};

use common::{Bench, Client};

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
    fs::create_dir(bench.at("dir")).unwrap();
    fs::write(bench.at("dir/in"), "in").unwrap();
    fs::create_dir(bench.at("into")).unwrap();
    let client = bench.client();
    let job = undoable(
        &client,
        nova_files_transfer_api::copy(vec![bench.at("a"), bench.at("dir")], bench.at("into")),
    );

    assert_eq!(undo(&client, job), Outcome::Done);

    assert!(!bench.at("into/a").exists() && !bench.at("into/dir").exists());
    assert!(bench.at("a").exists() && bench.at("dir/in").exists());
    let trash = home_trash_files(&bench.root);
    assert_eq!(fs::read_to_string(trash.join("a")).unwrap(), "a");
    assert_eq!(fs::read_to_string(trash.join("dir/in")).unwrap(), "in");
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
        nova_files_transfer_api::move_into(vec![bench.at("dir")], bench.at("into")),
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
    let job = client.start(nova_files_transfer_api::move_into(
        vec![bench.at("dir")],
        bench.at("into"),
    ));
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

fn replace_then_undo(test: &str, operation: fn(PathBuf, PathBuf) -> Operation) -> Bench {
    let bench = Bench::new(test);
    fs::write(bench.at("a"), "new").unwrap();
    fs::create_dir(bench.at("into")).unwrap();
    fs::write(bench.at("into/a"), "old").unwrap();
    let client = bench.client();
    let job = client.start(operation(bench.at("a"), bench.at("into")));
    let clash = answers::held(&client, job);
    answers::answer(&client, job, &clash, Answer::Replace, Scope::ThisOne);
    assert_eq!(client.ended(job).0, Outcome::Done);
    assert_eq!(fs::read_to_string(bench.at("into/a")).unwrap(), "new");
    let trash = home_trash_files(&bench.root);
    assert_eq!(fs::read_to_string(trash.join("a")).unwrap(), "old");

    assert_eq!(undo(&client, job), Outcome::Done);

    assert_eq!(fs::read_to_string(bench.at("into/a")).unwrap(), "old");
    bench
}

#[test]
fn undoing_a_copy_that_replaced_puts_the_replaced_file_back() {
    let bench = replace_then_undo("undo-copy-replace", |source, into| {
        nova_files_transfer_api::copy(vec![source], into)
    });

    assert_eq!(fs::read_to_string(bench.at("a")).unwrap(), "new");
    let trash = home_trash_files(&bench.root);
    let trashed: Vec<_> = fs::read_dir(&trash)
        .unwrap()
        .map(|entry| fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect();
    assert_eq!(trashed, ["new"], "only the copy is left in the trash");
}

#[test]
fn undoing_a_move_that_replaced_puts_both_back() {
    let bench = replace_then_undo("undo-move-replace", |source, into| {
        nova_files_transfer_api::move_into(vec![source], into)
    });

    assert_eq!(fs::read_to_string(bench.at("a")).unwrap(), "new");
    assert_eq!(
        fs::read_dir(home_trash_files(&bench.root)).unwrap().count(),
        0
    );
}

#[test]
fn undo_leaves_out_a_file_the_same_job_made_and_then_replaced() {
    let bench = Bench::new("undo-replace-own");
    fs::create_dir_all(bench.at("a")).unwrap();
    fs::create_dir_all(bench.at("b")).unwrap();
    fs::create_dir(bench.at("into")).unwrap();
    fs::write(bench.at("a/report"), "a").unwrap();
    fs::write(bench.at("b/report"), "b").unwrap();
    let client = bench.client();
    let job = client.start(nova_files_transfer_api::copy(
        vec![bench.at("a/report"), bench.at("b/report")],
        bench.at("into"),
    ));
    let clash = answers::held(&client, job);
    answers::answer(&client, job, &clash, Answer::Replace, Scope::ThisOne);
    assert_eq!(client.ended(job).0, Outcome::Done);
    assert_eq!(fs::read_to_string(bench.at("into/report")).unwrap(), "b");

    assert_eq!(undo(&client, job), Outcome::Done);

    assert!(
        !bench.at("into/report").exists(),
        "as it was before the job"
    );
}

#[test]
fn a_replaced_file_whose_name_was_taken_again_stays_in_the_trash() {
    let bench = Bench::new("undo-replace-taken");
    fs::write(bench.at("a"), "new").unwrap();
    fs::create_dir(bench.at("into")).unwrap();
    fs::write(bench.at("into/a"), "old").unwrap();
    let client = bench.client();
    let job = client.start(nova_files_transfer_api::copy(
        vec![bench.at("a")],
        bench.at("into"),
    ));
    let clash = answers::held(&client, job);
    answers::answer(&client, job, &clash, Answer::Replace, Scope::ThisOne);
    assert_eq!(client.ended(job).0, Outcome::Done);
    fs::remove_file(bench.at("into/a")).unwrap();
    fs::write(bench.at("into/a"), "edited").unwrap();

    assert_ne!(undo(&client, job), Outcome::Done);

    assert_eq!(fs::read_to_string(bench.at("into/a")).unwrap(), "edited");
    let trash = home_trash_files(&bench.root);
    assert_eq!(fs::read_to_string(trash.join("a")).unwrap(), "old");
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
        nova_files_transfer_api::copy(vec![bench.at("a")], bench.at("into")),
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
