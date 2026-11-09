//! File operations through the real core, against real temp folders: the
//! client that starts a job hears its id first, every client hears how it
//! ended, and nothing is ever replaced.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use nova_files_api::{Command, Connection, Event, Failure, JobId, JobTag, Operation, Outcome};

/// How long a test waits for the core before it counts as a failure.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);

/// A scratch folder for one test, and a core whose home is elsewhere.
struct Bench {
    root: PathBuf,
    core: nova_files_core::RunningCore,
}

impl Bench {
    fn new(test: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("nova-files-jobs-{test}-{}", std::process::id()));
        fs::remove_dir_all(&root).ok();
        fs::create_dir_all(root.join("home")).unwrap();
        let home = nova_files_core::Home {
            folder: root.join("home"),
            config: root.join("home/.config"),
            state: root.join("home/.local/state"),
        };
        let core = nova_files_core::spawn_with(nova_files_core::IDLE_EXIT, Some(home), None, None)
            .unwrap();
        Self { root, core }
    }

    fn at(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn client(&self) -> (Connection, Receiver<Event>) {
        let (sink, events) = mpsc::channel();
        (self.core.connector.connect(sink).unwrap(), events)
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).ok();
    }
}

/// Starts `operation` and returns its id and how it ended, with the
/// folders it changed.
fn run(
    connection: &Connection,
    events: &Receiver<Event>,
    operation: Operation,
) -> (JobId, Outcome, Vec<PathBuf>) {
    let tag = JobTag(7);
    connection
        .send(Command::StartJob { tag, operation })
        .unwrap();
    let Event::JobStarted { tag: got, job } = events.recv_timeout(ANSWER_TIMEOUT).unwrap() else {
        panic!("the job's id came first");
    };
    assert_eq!(got, tag);
    match events.recv_timeout(ANSWER_TIMEOUT).unwrap() {
        Event::JobFinished {
            job: ended,
            outcome,
            changed,
        } if ended == job => (job, outcome, changed),
        other => panic!("expected the job to end, got {other:?}"),
    }
}

fn rename(path: &Path, name: &str) -> Operation {
    Operation::Rename {
        path: path.to_owned(),
        name: name.into(),
    }
}

#[test]
fn renames_a_file_in_its_folder() {
    let bench = Bench::new("rename");
    fs::write(bench.at("a.txt"), "text").unwrap();
    let (connection, events) = bench.client();

    let (_, outcome, changed) = run(&connection, &events, rename(&bench.at("a.txt"), "b.txt"));

    assert_eq!(outcome, Outcome::Done);
    assert_eq!(changed, vec![bench.root.clone()]);
    assert!(!bench.at("a.txt").exists());
    assert_eq!(fs::read_to_string(bench.at("b.txt")).unwrap(), "text");
}

#[test]
fn renames_a_folder_and_a_name_that_isnt_utf8() {
    let bench = Bench::new("rename-odd");
    fs::create_dir(bench.at("dir")).unwrap();
    let (connection, events) = bench.client();
    let odd = OsStr::from_bytes(b"caf\xe9");

    let operation = Operation::Rename {
        path: bench.at("dir"),
        name: odd.to_owned(),
    };
    let (_, outcome, _) = run(&connection, &events, operation);

    assert_eq!(outcome, Outcome::Done);
    assert!(bench.root.join(odd).is_dir());
}

#[test]
fn a_rename_never_replaces_a_taken_name() {
    let bench = Bench::new("rename-taken");
    fs::write(bench.at("a"), "new").unwrap();
    fs::write(bench.at("b"), "old").unwrap();
    let (connection, events) = bench.client();

    let (_, outcome, changed) = run(&connection, &events, rename(&bench.at("a"), "b"));

    assert_eq!(
        outcome,
        Outcome::Failed {
            path: bench.at("a"),
            reason: Failure::NameTaken
        }
    );
    assert_eq!(changed, Vec::<PathBuf>::new());
    assert_eq!(fs::read_to_string(bench.at("a")).unwrap(), "new");
    assert_eq!(fs::read_to_string(bench.at("b")).unwrap(), "old");
}

#[test]
fn a_rename_to_its_own_name_changes_nothing() {
    let bench = Bench::new("rename-same");
    fs::write(bench.at("a"), "kept").unwrap();
    let (connection, events) = bench.client();

    let (_, outcome, changed) = run(&connection, &events, rename(&bench.at("a"), "a"));

    assert_eq!(outcome, Outcome::Done);
    assert_eq!(changed, Vec::<PathBuf>::new());
    assert_eq!(fs::read_to_string(bench.at("a")).unwrap(), "kept");
}

#[test]
fn a_name_that_leaves_the_folder_is_refused() {
    let bench = Bench::new("rename-escape");
    fs::create_dir(bench.at("sub")).unwrap();
    fs::write(bench.at("sub/a"), "kept").unwrap();
    let (connection, events) = bench.client();

    for name in ["../a", "..", "", "x/y"] {
        let (_, outcome, _) = run(&connection, &events, rename(&bench.at("sub/a"), name));
        assert_eq!(
            outcome,
            Outcome::Failed {
                path: bench.at("sub/a"),
                reason: Failure::InvalidName
            },
            "{name:?}"
        );
    }
    assert!(bench.at("sub/a").exists());
    assert!(!bench.at("a").exists());
}

#[test]
fn renaming_the_root_or_a_vanished_file_fails() {
    let bench = Bench::new("rename-missing");
    let (connection, events) = bench.client();

    let (_, root, _) = run(&connection, &events, rename(Path::new("/"), "x"));
    let (_, gone, _) = run(&connection, &events, rename(&bench.at("gone"), "x"));
    let (_, gone_same, _) = run(&connection, &events, rename(&bench.at("gone"), "gone"));
    let (_, relative, _) = run(&connection, &events, rename(Path::new("a/b"), "c"));
    let (_, relative_folder, _) = run(
        &connection,
        &events,
        Operation::NewFolder {
            inside: "a".into(),
            name: "b".into(),
        },
    );

    assert_eq!(
        root,
        Outcome::Failed {
            path: "/".into(),
            reason: Failure::InvalidName
        }
    );
    assert_eq!(
        gone,
        Outcome::Failed {
            path: bench.at("gone"),
            reason: Failure::NotFound
        }
    );
    assert_eq!(gone_same, gone);
    assert_eq!(
        relative,
        Outcome::Failed {
            path: "a/b".into(),
            reason: Failure::InvalidName
        }
    );
    assert_eq!(
        relative_folder,
        Outcome::Failed {
            path: "a/b".into(),
            reason: Failure::InvalidName
        }
    );
}

#[test]
fn makes_a_new_folder_and_never_replaces() {
    let bench = Bench::new("new-folder");
    fs::write(bench.at("taken"), "old").unwrap();
    let (connection, events) = bench.client();
    let new_folder = |name: &str| Operation::NewFolder {
        inside: bench.root.clone(),
        name: OsString::from(name),
    };

    let (first, made, changed) = run(&connection, &events, new_folder("Photos"));
    let (second, taken, _) = run(&connection, &events, new_folder("taken"));
    let (_, invalid, _) = run(&connection, &events, new_folder("a/b"));

    assert_ne!(first, second);
    assert_eq!(made, Outcome::Done);
    assert_eq!(changed, vec![bench.root.clone()]);
    assert!(bench.at("Photos").is_dir());
    assert_eq!(
        taken,
        Outcome::Failed {
            path: bench.at("taken"),
            reason: Failure::NameTaken
        }
    );
    assert_eq!(fs::read_to_string(bench.at("taken")).unwrap(), "old");
    assert_eq!(
        invalid,
        Outcome::Failed {
            path: bench.at("a/b"),
            reason: Failure::InvalidName
        }
    );
}

#[test]
fn every_watching_client_hears_a_job_end_but_only_the_asker_hears_it_start() {
    let bench = Bench::new("audience");
    let (watcher, heard_by_watcher) = bench.client();
    let (connection, events) = bench.client();
    // The watcher watches once the core has answered after it asked.
    watcher.send(Command::WatchJobs).unwrap();
    watcher.send(Command::Ping).unwrap();
    assert_eq!(
        heard_by_watcher.recv_timeout(ANSWER_TIMEOUT).unwrap(),
        Event::Pong
    );

    let (job, _, _) = run(
        &connection,
        &events,
        Operation::NewFolder {
            inside: bench.root.clone(),
            name: "x".into(),
        },
    );

    let Event::JobFinished { job: heard, .. } =
        heard_by_watcher.recv_timeout(ANSWER_TIMEOUT).unwrap()
    else {
        panic!("the watcher heard the job end, and nothing before");
    };
    assert_eq!(heard, job);
}

#[test]
fn a_client_that_left_hears_nothing_more() {
    let bench = Bench::new("left");
    let (gone, gone_events) = bench.client();
    gone.send(Command::WatchJobs).unwrap();
    drop(gone);
    let (connection, events) = bench.client();

    run(
        &connection,
        &events,
        Operation::NewFolder {
            inside: bench.root.clone(),
            name: "x".into(),
        },
    );

    assert!(gone_events.try_recv().is_err());
}

#[test]
fn a_client_that_never_watched_hears_nothing() {
    let bench = Bench::new("bystander");
    let (bystander, heard) = bench.client();
    bystander.send(Command::Ping).unwrap();
    assert_eq!(heard.recv_timeout(ANSWER_TIMEOUT).unwrap(), Event::Pong);
    let (connection, events) = bench.client();

    run(
        &connection,
        &events,
        Operation::NewFolder {
            inside: bench.root.clone(),
            name: "x".into(),
        },
    );
    bystander.send(Command::Ping).unwrap();

    assert_eq!(heard.recv_timeout(ANSWER_TIMEOUT).unwrap(), Event::Pong);
}
