#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

#[path = "common/answers.rs"]
mod answers;
mod common;

use std::fs;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use std::sync::mpsc;

use nova_files_api::{
    Answer, Command, Conflict, ConflictKind, EntryKind, Event, Failure, JobId, Operation, Outcome,
    Scope,
};
use nova_files_trash_api::{self as trash_api, Trashed};

use answers::{answer, held};
use common::{ANSWER_TIMEOUT, Bench, Client};

fn home_trash(root: &Path) -> PathBuf {
    root.join("home/.local/share/Trash")
}

fn listed(bench: &Bench) -> Vec<Trashed> {
    let (sink, events) = mpsc::channel();
    let connection = bench.core.connector.connect(sink).unwrap();
    connection.send(trash_api::Command::List).unwrap();
    let trash_api::Event::Listed {
        items: mut listed, ..
    } = events.recv_timeout(ANSWER_TIMEOUT).unwrap();
    listed.sort_by(|a, b| a.item.cmp(&b.item));
    listed
}

fn trashed(bench: &Bench, client: &Client, paths: &[PathBuf]) -> Vec<PathBuf> {
    let (_, outcome, _) = client.run(Operation::Trash {
        paths: paths.to_vec(),
    });
    assert_eq!(outcome, Outcome::Done);
    listed(bench).into_iter().map(|entry| entry.item).collect()
}

fn info_of(item: &Path) -> PathBuf {
    let name = item.file_name().unwrap().to_str().unwrap();
    item.parent()
        .unwrap()
        .with_file_name("info")
        .join(format!("{name}.trashinfo"))
}

#[test]
fn the_trash_lists_what_was_trashed_and_where_from() {
    let bench = Bench::new("restore-list");
    fs::write(bench.at("a"), "a").unwrap();
    fs::create_dir(bench.at("dir")).unwrap();
    let client = bench.client();
    assert_eq!(listed(&bench), vec![]);

    trashed(&bench, &client, &[bench.at("a"), bench.at("dir")]);

    let files = home_trash(&bench.root)
        .canonicalize()
        .unwrap()
        .join("files");
    let real = bench.root.canonicalize().unwrap();
    let listed = listed(&bench);
    assert_eq!(listed.len(), 2);
    assert_eq!(
        (&listed[0].item, &listed[0].original, listed[0].kind),
        (&files.join("a"), &real.join("a"), EntryKind::File)
    );
    assert_eq!(listed[1].kind, EntryKind::Folder);
    assert!(listed.iter().all(|entry| entry.deleted.is_some()));
}

#[test]
fn a_restore_puts_entries_back_and_their_info_files_go() {
    let bench = Bench::new("restore-back");
    fs::write(bench.at("a"), "a").unwrap();
    fs::create_dir_all(bench.at("dir/sub")).unwrap();
    fs::write(bench.at("dir/sub/f"), "f").unwrap();
    let client = bench.client();
    let items = trashed(&bench, &client, &[bench.at("a"), bench.at("dir")]);

    let (_, outcome, changed) = client.run(Operation::Restore {
        items: items.clone(),
    });

    assert_eq!(outcome, Outcome::Done);
    assert_eq!(fs::read_to_string(bench.at("a")).unwrap(), "a");
    assert_eq!(fs::read_to_string(bench.at("dir/sub/f")).unwrap(), "f");
    assert!(
        items
            .iter()
            .all(|item| !item.exists() && !info_of(item).exists())
    );
    assert_eq!(listed(&bench), vec![]);
    let real = bench.root.canonicalize().unwrap();
    let trash = home_trash(&bench.root).canonicalize().unwrap();
    assert_eq!(changed, vec![real, trash]);
}

#[test]
fn a_restore_makes_the_folders_that_are_gone() {
    let bench = Bench::new("restore-folders");
    fs::create_dir_all(bench.at("x/y")).unwrap();
    fs::write(bench.at("x/y/a"), "a").unwrap();
    let client = bench.client();
    let items = trashed(&bench, &client, &[bench.at("x/y/a")]);
    fs::remove_dir_all(bench.at("x")).unwrap();

    let (_, outcome, _) = client.run(Operation::Restore { items });

    assert_eq!(outcome, Outcome::Done);
    assert_eq!(fs::read_to_string(bench.at("x/y/a")).unwrap(), "a");
}

fn restore_onto_a_taken_name(name: &str) -> (Bench, Client, Vec<PathBuf>, JobId, Conflict) {
    let bench = Bench::new(name);
    fs::write(bench.at("a"), "trashed").unwrap();
    let client = bench.client();
    let items = trashed(&bench, &client, &[bench.at("a")]);
    fs::write(bench.at("a"), "new").unwrap();
    let job = client.start(Operation::Restore {
        items: items.clone(),
    });
    let conflict = held(&client, job);
    (bench, client, items, job, conflict)
}

#[test]
fn a_name_taken_where_it_goes_back_asks_and_skip_keeps_it_in_the_trash() {
    let (bench, client, items, job, conflict) = restore_onto_a_taken_name("restore-skip");
    assert_eq!(conflict.kind, ConflictKind::FileExists);
    answer(&client, job, &conflict, Answer::Skip, Scope::ThisOne);

    assert_eq!(client.ended(job).0, Outcome::Done);
    assert_eq!(fs::read_to_string(bench.at("a")).unwrap(), "new");
    assert!(items[0].exists() && info_of(&items[0]).exists());
    assert_eq!(listed(&bench).len(), 1);
}

#[test]
fn keep_both_puts_it_back_beside_what_took_its_name() {
    let (bench, client, items, job, conflict) = restore_onto_a_taken_name("restore-both");
    answer(&client, job, &conflict, Answer::KeepBoth, Scope::ThisOne);

    assert_eq!(client.ended(job).0, Outcome::Done);
    assert_eq!(fs::read_to_string(bench.at("a")).unwrap(), "new");
    assert!(!items[0].exists() && !info_of(&items[0]).exists());
    assert_eq!(listed(&bench), vec![]);
}

#[test]
fn what_isnt_in_a_trash_fails_the_job_with_nothing_changed() {
    let bench = Bench::new("restore-not-trashed");
    fs::write(bench.at("a"), "a").unwrap();
    fs::write(bench.at("b"), "b").unwrap();
    let client = bench.client();
    let items = trashed(&bench, &client, &[bench.at("a")]);

    for operation in [
        Operation::Restore {
            items: vec![items[0].clone(), bench.at("b")],
        },
        Operation::DeleteFromTrash {
            items: vec![items[0].clone(), bench.at("b")],
        },
    ] {
        let (_, outcome, _) = client.run(operation);

        assert_eq!(
            outcome,
            Outcome::Failed {
                path: bench.at("b"),
                reason: Failure::Unsupported
            }
        );
    }
    assert!(items[0].exists() && bench.at("b").exists());
    let (_, gone, _) = client.run(Operation::Restore {
        items: vec![bench.at("c")],
    });
    assert!(matches!(
        gone,
        Outcome::Failed {
            reason: Failure::NotFound,
            ..
        }
    ));
}

#[test]
fn an_entry_without_an_info_file_is_not_the_trashs_to_restore() {
    let bench = Bench::new("restore-stray");
    let files = home_trash(&bench.root).join("files");
    fs::create_dir_all(&files).unwrap();
    fs::create_dir_all(home_trash(&bench.root).join("info")).unwrap();
    fs::write(files.join("stray"), "theirs").unwrap();
    let client = bench.client();

    let (_, outcome, _) = client.run(Operation::DeleteFromTrash {
        items: vec![files.canonicalize().unwrap().join("stray")],
    });

    assert!(matches!(
        outcome,
        Outcome::Failed {
            reason: Failure::Unsupported,
            ..
        }
    ));
    assert!(files.join("stray").exists());
}

#[test]
fn deleting_from_the_trash_removes_entries_and_their_info_files() {
    let bench = Bench::new("restore-delete");
    fs::write(bench.at("a"), "a").unwrap();
    fs::create_dir_all(bench.at("dir/sub")).unwrap();
    fs::write(bench.at("dir/sub/f"), "f").unwrap();
    fs::write(bench.at("kept"), "kept").unwrap();
    let client = bench.client();
    let items = trashed(
        &bench,
        &client,
        &[bench.at("a"), bench.at("dir"), bench.at("kept")],
    );
    let gone: Vec<_> = items
        .iter()
        .filter(|item| !item.ends_with("kept"))
        .cloned()
        .collect();

    let (_, outcome, changed) = client.run(Operation::DeleteFromTrash {
        items: gone.clone(),
    });

    assert_eq!(outcome, Outcome::Done);
    assert!(
        gone.iter()
            .all(|item| !item.exists() && !info_of(item).exists())
    );
    let left = listed(&bench);
    assert_eq!(left.len(), 1);
    assert!(left[0].item.ends_with("kept"));
    let trash = home_trash(&bench.root).canonicalize().unwrap();
    assert_eq!(changed, vec![trash]);
}

#[test]
fn emptying_the_trash_deletes_all_it_lists() {
    let bench = Bench::new("restore-empty");
    fs::write(bench.at("a"), "a").unwrap();
    fs::create_dir(bench.at("dir")).unwrap();
    let client = bench.client();
    let items = trashed(&bench, &client, &[bench.at("a"), bench.at("dir")]);
    let stray = items[0].with_file_name("stray");
    fs::write(&stray, "theirs").unwrap();

    let (_, outcome, _) = client.run(Operation::EmptyTrash);

    assert_eq!(outcome, Outcome::Done);
    assert_eq!(listed(&bench), vec![]);
    assert!(
        items
            .iter()
            .all(|item| !item.exists() && !info_of(item).exists())
    );
    assert!(stray.exists(), "only what the trash lists goes");
    let (_, again, changed) = client.run(Operation::EmptyTrash);
    assert_eq!((again, changed), (Outcome::Done, vec![]));
}

fn crashed_restore(name: &str, moved: bool) -> Bench {
    Bench::with(name, nova_files_core::IDLE_EXIT, |root| {
        let trash = home_trash(root);
        fs::create_dir_all(trash.join("info")).unwrap();
        fs::create_dir_all(trash.join("files")).unwrap();
        let trash = trash.canonicalize().unwrap();
        let (item, info) = (trash.join("files/a"), trash.join("info/a.trashinfo"));
        fs::write(&item, "trashed").unwrap();
        fs::write(
            &info,
            format!("[Trash Info]\nPath={}\n", root.join("a").display()),
        )
        .unwrap();
        let entry = |path: &Path| {
            let there = path.symlink_metadata().unwrap();
            format!("{}:{}", there.dev(), there.ino())
        };
        let text = format!(
            "nova-files-job 1\nrestore\nsource {}\nsources-done\ntrash-info {} {} {}\n",
            item.display(),
            info.display(),
            entry(&info),
            entry(&item),
        );
        if moved {
            fs::rename(&item, root.join("a")).unwrap();
        }
        let journals = root.join("home/.local/state/nova-files/jobs");
        fs::create_dir_all(&journals).unwrap();
        fs::write(journals.join("42"), text).unwrap();
    })
}

#[test]
fn a_crash_after_the_entry_went_back_leaves_no_info_file() {
    let bench = crashed_restore("restore-crash-moved", true);
    let client = bench.client();
    client.send(Command::WatchJobs);

    let interrupted = client.next();

    assert!(matches!(interrupted, Event::JobInterrupted { .. }));
    let trash = home_trash(&bench.root);
    assert!(!trash.join("info/a.trashinfo").exists());
    assert_eq!(fs::read_to_string(bench.at("a")).unwrap(), "trashed");
    client.send(Command::ResumeJob(JobId(42)));
    assert_eq!(client.next(), Event::JobResumed(JobId(42)));
    assert_eq!(client.ended(JobId(42)).0, Outcome::Done);
}

#[test]
fn a_crash_before_the_entry_went_back_keeps_it_listed_and_resumes() {
    let bench = crashed_restore("restore-crash-before", false);
    let client = bench.client();
    client.send(Command::WatchJobs);
    let job = JobId(42);

    let interrupted = client.next();

    let item = home_trash(&bench.root)
        .canonicalize()
        .unwrap()
        .join("files/a");
    assert_eq!(
        interrupted,
        Event::JobInterrupted {
            job,
            operation: Operation::Restore {
                items: vec![item.clone()]
            }
        }
    );
    assert_eq!(listed(&bench).len(), 1);
    client.send(Command::ResumeJob(job));
    assert_eq!(client.next(), Event::JobResumed(job));
    assert_eq!(client.ended(job).0, Outcome::Done);
    assert_eq!(fs::read_to_string(bench.at("a")).unwrap(), "trashed");
    assert!(!item.exists() && !info_of(&item).exists());
}

#[test]
fn a_folder_and_what_was_in_it_go_back_together() {
    let bench = Bench::new("restore-nested");
    fs::create_dir(bench.at("a")).unwrap();
    fs::write(bench.at("a/b.txt"), "b").unwrap();
    fs::write(bench.at("a/c.txt"), "c").unwrap();
    let client = bench.client();
    trashed(&bench, &client, &[bench.at("a/b.txt")]);
    let items = trashed(&bench, &client, &[bench.at("a")]);

    let (_, outcome, _) = client.run(Operation::Restore {
        items: items.into_iter().rev().collect(),
    });

    assert_eq!(outcome, Outcome::Done, "nothing to ask about");
    assert_eq!(fs::read_to_string(bench.at("a/b.txt")).unwrap(), "b");
    assert_eq!(fs::read_to_string(bench.at("a/c.txt")).unwrap(), "c");
    assert_eq!(listed(&bench), vec![]);
}

#[test]
fn a_resumed_restore_leaves_what_took_a_freed_name_since() {
    let bench = crashed_restore("restore-crash-newcomer", true);
    let trash = home_trash(&bench.root).canonicalize().unwrap();
    let client = bench.client();
    client.send(Command::WatchJobs);
    assert!(matches!(client.next(), Event::JobInterrupted { .. }));
    fs::write(trash.join("files/a"), "newcomer").unwrap();
    fs::write(
        trash.join("info/a.trashinfo"),
        format!("[Trash Info]\nPath={}\n", bench.at("b").display()),
    )
    .unwrap();

    client.send(Command::ResumeJob(JobId(42)));

    assert_eq!(client.next(), Event::JobResumed(JobId(42)));
    assert_eq!(client.ended(JobId(42)).0, Outcome::Done);
    assert_eq!(
        fs::read_to_string(trash.join("files/a")).unwrap(),
        "newcomer"
    );
    assert!(trash.join("info/a.trashinfo").exists());
    assert_eq!(fs::read_to_string(bench.at("a")).unwrap(), "trashed");
    assert!(!bench.at("b").exists());
}
