//! Resuming after a crash, through the real core: a core that finds a job
//! journal cleans up what the job left half-done, lists the job as
//! interrupted, and resumes or forgets it when a client asks.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use nova_files_api::{Command, Event, JobId, Operation, Outcome};

use common::{Bench, Client};

/// The id the crashed job had.
const CRASHED: JobId = JobId(42);

/// Where the core under `root` keeps job journals.
fn journals(root: &Path) -> PathBuf {
    root.join("home/.local/state/nova-files/jobs")
}

/// Leaves a journal of `lines` under `root` for job [`CRASHED`], as a
/// crash would, and returns where.
fn crashed_job(root: &Path, lines: &[String]) -> PathBuf {
    fs::create_dir_all(journals(root)).unwrap();
    let journal = journals(root).join(CRASHED.0.to_string());
    let mut text = String::from("nova-files-job 1\n");
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    fs::write(&journal, text).unwrap();
    journal
}

/// A core started after a crash that `prepare` sets up, and a client of
/// it that watches jobs.
fn restart(test: &str, prepare: impl FnOnce(&Path)) -> (Bench, Client) {
    let bench = Bench::with(test, nova_files_core::IDLE_EXIT, prepare);
    let client = bench.client();
    client.send(Command::WatchJobs);
    (bench, client)
}

fn line(word: &str, paths: &[&Path]) -> String {
    let mut line = word.to_owned();
    for path in paths {
        line.push(' ');
        line.push_str(path.to_str().unwrap());
    }
    line
}

#[test]
fn a_crashed_copy_is_cleaned_up_and_resumes_where_it_left_off() {
    let (bench, client) = restart("resume-copy", |root| {
        let (source, into) = (root.join("src"), root.join("into"));
        fs::create_dir(&source).unwrap();
        fs::write(source.join("a"), "a").unwrap();
        fs::write(source.join("b"), "b").unwrap();
        fs::create_dir_all(into.join("src")).unwrap();
        // Made before the crash: taken as done, not asked about.
        fs::write(into.join("src/a"), "a").unwrap();
        fs::write(into.join("src/.nova-part-42-2"), "half").unwrap();
        crashed_job(
            root,
            &[
                "copy".to_owned(),
                line("into", &[&into]),
                line("source", &[&source]),
                line("made", &[&into.join("src")]),
                line("made", &[&into.join("src/a")]),
                line("part", &[&into.join("src/.nova-part-42-2")]),
            ],
        );
    });
    let (source, into) = (bench.at("src"), bench.at("into"));

    let operation = Operation::Copy {
        sources: vec![source],
        into: into.clone(),
    };
    assert_eq!(
        client.next(),
        Event::JobInterrupted {
            job: CRASHED,
            operation
        }
    );
    assert!(
        !into.join("src/.nova-part-42-2").exists(),
        "half a copy goes at once"
    );
    client.send(Command::ResumeJob(CRASHED));
    assert_eq!(client.next(), Event::JobResumed(CRASHED));
    let (outcome, _) = client.ended(CRASHED);
    assert_eq!(outcome, Outcome::Done);
    assert_eq!(fs::read_to_string(into.join("src/b")).unwrap(), "b");
    assert!(
        !journals(&bench.root).join("42").exists(),
        "a finished job's journal goes"
    );
}

#[test]
fn a_file_that_stepped_aside_comes_back_unless_its_replacement_is_in() {
    let (bench, _client) = restart("resume-backup", |root| {
        let into = root.join("into");
        fs::create_dir(&into).unwrap();
        fs::write(into.join(".nova-backup-42-0"), "old").unwrap();
        fs::write(into.join("replaced"), "new").unwrap();
        fs::write(into.join(".nova-backup-42-1"), "older").unwrap();
        crashed_job(
            root,
            &[
                "copy".to_owned(),
                line("into", &[&into]),
                line(
                    "backup",
                    &[&into.join(".nova-backup-42-0"), &into.join("back")],
                ),
                line(
                    "backup",
                    &[&into.join(".nova-backup-42-1"), &into.join("replaced")],
                ),
            ],
        );
    });
    let into = bench.at("into");

    assert_eq!(fs::read_to_string(into.join("back")).unwrap(), "old");
    assert_eq!(fs::read_to_string(into.join("replaced")).unwrap(), "new");
    assert!(!into.join(".nova-backup-42-0").exists());
    assert!(!into.join(".nova-backup-42-1").exists());
}

#[test]
fn a_forgotten_job_ends_and_its_journal_goes() {
    let (bench, client) = restart("resume-forget", |root| {
        fs::write(root.join("kept"), "kept").unwrap();
        crashed_job(
            root,
            &["delete".to_owned(), line("source", &[&root.join("kept")])],
        );
    });

    assert!(matches!(client.next(), Event::JobInterrupted { .. }));
    client.send(Command::ForgetJob(CRASHED));

    assert_eq!(
        client.ended(CRASHED),
        (Outcome::Cancelled, Vec::<PathBuf>::new())
    );
    assert!(!journals(&bench.root).join("42").exists());
    assert!(bench.at("kept").exists(), "forgetting does nothing more");
}

#[test]
fn a_resumed_move_moves_only_what_is_still_there() {
    let (bench, client) = restart("resume-move", |root| {
        let into = root.join("into");
        fs::create_dir(&into).unwrap();
        // `a` moved before the crash; `b` didn't.
        fs::write(into.join("a"), "a").unwrap();
        fs::write(root.join("b"), "b").unwrap();
        crashed_job(
            root,
            &[
                "move".to_owned(),
                line("into", &[&into]),
                line("source", &[&root.join("a")]),
                line("source", &[&root.join("b")]),
                line("moved", &[&root.join("a"), &into.join("a")]),
            ],
        );
    });

    assert!(matches!(client.next(), Event::JobInterrupted { .. }));
    client.send(Command::ResumeJob(CRASHED));

    assert_eq!(client.next(), Event::JobResumed(CRASHED));
    assert_eq!(client.ended(CRASHED).0, Outcome::Done);
    assert_eq!(fs::read_to_string(bench.at("into/b")).unwrap(), "b");
    assert!(!bench.at("b").exists());
}

#[test]
fn a_journal_that_is_not_one_is_left_alone() {
    let (bench, client) = restart("resume-garbage", |root| {
        fs::create_dir_all(journals(root)).unwrap();
        fs::write(journals(root).join("7"), "not a journal\n").unwrap();
    });

    // Its id comes first: no job was listed before it.
    let (_, outcome, _) = client.run(Operation::Delete { paths: Vec::new() });

    assert_eq!(outcome, Outcome::Done);
    assert!(journals(&bench.root).join("7").exists());
}

#[test]
fn a_core_with_no_journals_lists_no_job() {
    let bench = Bench::new("resume-none");
    let client = bench.client();
    client.send(Command::WatchJobs);

    let (_, outcome, _) = client.run(Operation::Delete { paths: Vec::new() });

    assert_eq!(outcome, Outcome::Done);
    let left = fs::read_dir(journals(&bench.root)).unwrap().count();
    assert_eq!(left, 0, "the delete's own journal went when it ended");
}
