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
    crashed(root, CRASHED, lines)
}

/// Leaves a journal of `lines` under `root` for job `job`.
fn crashed(root: &Path, job: JobId, lines: &[String]) -> PathBuf {
    fs::create_dir_all(journals(root)).unwrap();
    let journal = journals(root).join(job.0.to_string());
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
        let path = path.to_str().unwrap();
        line.push_str(&path.replace('%', "%25").replace(' ', "%20"));
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
                line("made", &[&into.join("src"), &source]),
                line("made", &[&into.join("src/a"), &source.join("a")]),
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
    let (bench, client) = restart("resume-backup", |root| {
        let into = root.join("into");
        fs::create_dir(&into).unwrap();
        fs::write(into.join(".nova-backup-42-0"), "old").unwrap();
        fs::write(into.join("replaced"), "new").unwrap();
        fs::write(into.join(".nova-backup-42-1"), "older").unwrap();
        // Something else took this name; the job's copy never did.
        fs::write(into.join("taken"), "theirs").unwrap();
        fs::write(into.join(".nova-backup-42-2"), "mine").unwrap();
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
                line("made", &[&into.join("replaced"), &root.join("replaced")]),
                line(
                    "backup",
                    &[&into.join(".nova-backup-42-2"), &into.join("taken")],
                ),
            ],
        );
    });
    let into = bench.at("into");

    assert!(matches!(client.next(), Event::JobInterrupted { .. }));
    assert_eq!(fs::read_to_string(into.join("back")).unwrap(), "old");
    assert_eq!(fs::read_to_string(into.join("replaced")).unwrap(), "new");
    assert!(!into.join(".nova-backup-42-0").exists());
    assert!(!into.join(".nova-backup-42-1").exists());
    assert_eq!(fs::read_to_string(into.join("taken")).unwrap(), "theirs");
    assert_eq!(
        fs::read_to_string(into.join(".nova-backup-42-2")).unwrap(),
        "mine",
        "the user's file stays hidden rather than lost"
    );
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

#[test]
fn a_resumed_copy_finds_what_it_made_under_another_name() {
    let (bench, client) = restart("resume-kept-both", |root| {
        let (source, into) = (root.join("src"), root.join("into"));
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("a"), "new").unwrap();
        fs::create_dir(&into).unwrap();
        // The user kept both before the crash.
        fs::write(into.join("a"), "old").unwrap();
        fs::write(into.join("a (2)"), "new").unwrap();
        crashed_job(
            root,
            &[
                "copy".to_owned(),
                line("into", &[&into]),
                line("source", &[&source.join("a")]),
                line("made", &[&into.join("a (2)"), &source.join("a")]),
            ],
        );
    });
    let into = bench.at("into");

    assert!(matches!(client.next(), Event::JobInterrupted { .. }));
    client.send(Command::ResumeJob(CRASHED));

    assert_eq!(client.next(), Event::JobResumed(CRASHED));
    // Not asked about `a` again: the job ends.
    assert_eq!(client.ended(CRASHED).0, Outcome::Done);
    assert_eq!(fs::read_to_string(into.join("a")).unwrap(), "old");
    assert!(!into.join("a (3)").exists());
}

#[test]
fn a_resumed_copy_makes_again_what_was_removed_since() {
    let (bench, client) = restart("resume-removed", |root| {
        let (source, into) = (root.join("src"), root.join("into"));
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("a"), "a").unwrap();
        fs::create_dir(&into).unwrap();
        crashed_job(
            root,
            &[
                "copy".to_owned(),
                line("into", &[&into]),
                line("source", &[&source.join("a")]),
                line("made", &[&into.join("a"), &source.join("a")]),
            ],
        );
    });

    assert!(matches!(client.next(), Event::JobInterrupted { .. }));
    client.send(Command::ResumeJob(CRASHED));

    assert_eq!(client.next(), Event::JobResumed(CRASHED));
    assert_eq!(client.ended(CRASHED).0, Outcome::Done);
    assert_eq!(fs::read_to_string(bench.at("into/a")).unwrap(), "a");
}

#[test]
fn a_resumed_delete_keeps_what_was_added_since() {
    let (bench, client) = restart("resume-delete", |root| {
        let tree = root.join("tree");
        fs::create_dir(&tree).unwrap();
        fs::write(tree.join("planned"), "x").unwrap();
        crashed_job(
            root,
            &[
                "delete".to_owned(),
                line("source", &[&tree]),
                line("planned", &[&tree]),
                line("planned", &[&tree.join("planned")]),
                "plan-done".to_owned(),
            ],
        );
        fs::write(tree.join("added"), "new").unwrap();
    });

    assert!(matches!(client.next(), Event::JobInterrupted { .. }));
    client.send(Command::ResumeJob(CRASHED));

    assert_eq!(client.next(), Event::JobResumed(CRASHED));
    assert_eq!(client.ended(CRASHED).0, Outcome::Done);
    assert!(!bench.at("tree/planned").exists());
    assert_eq!(fs::read_to_string(bench.at("tree/added")).unwrap(), "new");
}

#[test]
fn a_resumed_copy_skips_only_the_source_it_made() {
    let (bench, client) = restart("resume-same-name", |root| {
        let into = root.join("into");
        for (folder, text) in [("a", "first"), ("b", "second")] {
            fs::create_dir(root.join(folder)).unwrap();
            fs::write(root.join(folder).join("report"), text).unwrap();
        }
        fs::create_dir(&into).unwrap();
        fs::write(into.join("report"), "first").unwrap();
        crashed_job(
            root,
            &[
                "copy".to_owned(),
                line("into", &[&into]),
                line("source", &[&root.join("a/report")]),
                line("source", &[&root.join("b/report")]),
                line("made", &[&into.join("report"), &root.join("a/report")]),
            ],
        );
    });
    let job = CRASHED;

    assert!(matches!(client.next(), Event::JobInterrupted { .. }));
    client.send(Command::ResumeJob(job));
    assert_eq!(client.next(), Event::JobResumed(job));
    // The second report still meets the first one's name.
    let conflict = loop {
        match client.next() {
            Event::JobConflict { conflict, .. } => break conflict,
            Event::JobProgress { .. } => {}
            other => panic!("the second source is asked about, got {other:?}"),
        }
    };
    assert_eq!(conflict.source, bench.at("b/report"));
    client.send(Command::CancelJob(job));
    assert_eq!(client.ended(job).0, Outcome::Cancelled);
}

#[test]
fn a_delete_plan_cut_short_is_planned_again() {
    let (bench, client) = restart("resume-plan-cut", |root| {
        let tree = root.join("tree");
        fs::create_dir(&tree).unwrap();
        fs::write(tree.join("a"), "a").unwrap();
        crashed_job(
            root,
            &[
                "delete".to_owned(),
                line("source", &[&tree]),
                line("planned", &[&tree]),
            ],
        );
    });

    assert!(matches!(client.next(), Event::JobInterrupted { .. }));
    client.send(Command::ResumeJob(CRASHED));

    assert_eq!(client.next(), Event::JobResumed(CRASHED));
    assert_eq!(client.ended(CRASHED).0, Outcome::Done);
    assert!(!bench.at("tree").exists());
}

#[test]
fn a_journal_another_core_holds_is_left_alone() {
    let held = std::cell::RefCell::new(None);
    let (bench, client) = restart("resume-held", |root| {
        let part = root.join(".nova-part-42-0");
        fs::write(&part, "being written").unwrap();
        let journal = crashed_job(root, &["delete".to_owned(), line("part", &[&part])]);
        let file = fs::File::open(journal).unwrap();
        rustix::fs::flock(&file, rustix::fs::FlockOperation::LockExclusive).unwrap();
        *held.borrow_mut() = Some(file);
        crashed(root, JobId(43), &["delete".to_owned()]);
    });

    // Only the job whose journal nobody holds is listed.
    assert_eq!(
        client.next(),
        Event::JobInterrupted {
            job: JobId(43),
            operation: Operation::Delete { paths: Vec::new() }
        }
    );
    let (_, outcome, _) = client.run(Operation::Delete { paths: Vec::new() });
    assert_eq!(outcome, Outcome::Done);
    assert!(bench.at(".nova-part-42-0").exists());
    drop(held);
}
