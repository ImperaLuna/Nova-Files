//! Moving to the trash through the real core: entries go to the home
//! trash with info files every other app reads, nothing in the trash is
//! ever replaced, and a crash leaves no info file naming nothing.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

mod common;

use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _, symlink};
use std::path::{Path, PathBuf};

use nova_files_api::{
    Answer, Command, ConflictKind, Event, Failure, JobId, Operation, Outcome, Scope,
};

use common::Bench;

const fn trash(paths: Vec<PathBuf>) -> Operation {
    Operation::Trash { paths }
}

/// The home trash of the core under `root`.
fn home_trash(root: &Path) -> PathBuf {
    root.join("home/.local/share/Trash")
}

/// The info file the home trash keeps for `stored`.
fn info(bench: &Bench, stored: &str) -> String {
    let path = home_trash(&bench.root).join(format!("info/{stored}.trashinfo"));
    fs::read_to_string(path).unwrap()
}

/// The `Path=` an info file holds for `path`, escaped as it writes it.
fn path_line(path: &Path) -> String {
    let real = path.parent().unwrap().canonicalize().unwrap();
    let path = real.join(path.file_name().unwrap());
    format!("Path={}\n", path.to_str().unwrap().replace(' ', "%20"))
}

#[test]
fn files_and_folders_go_to_the_home_trash_with_their_info() {
    let bench = Bench::new("trash-home");
    let file = bench.at("notes.txt");
    fs::write(&file, "notes").unwrap();
    let folder = bench.at("My Photos");
    fs::create_dir_all(folder.join("2026")).unwrap();
    fs::write(folder.join("2026/a.jpg"), "12345").unwrap();
    let client = bench.client();

    let (_, outcome, changed) = client.run(trash(vec![file.clone(), folder.clone()]));

    assert_eq!(outcome, Outcome::Done);
    let trash = home_trash(&bench.root).canonicalize().unwrap();
    assert!(!file.exists() && !folder.exists());
    assert_eq!(
        fs::read_to_string(trash.join("files/notes.txt")).unwrap(),
        "notes"
    );
    assert!(trash.join("files/My Photos/2026/a.jpg").exists());
    let text = info(&bench, "notes.txt");
    assert!(text.starts_with(&format!("[Trash Info]\n{}", path_line(&file))));
    let date = text.lines().nth(2).unwrap();
    assert!(date.starts_with("DeletionDate=20") && date.len() == "DeletionDate=".len() + 19);
    assert!(info(&bench, "My Photos").contains(&path_line(&folder)));
    let sizes = fs::read_to_string(trash.join("directorysizes")).unwrap();
    assert!(
        sizes.starts_with("5 ") && sizes.ends_with(" My%20Photos\n"),
        "{sizes}"
    );
    for folder in [bench.root.clone(), trash.join("files"), trash.join("info")] {
        assert!(
            changed.contains(&folder),
            "{} in {changed:?}",
            folder.display()
        );
    }
}

#[test]
fn a_name_already_in_the_trash_is_never_replaced() {
    let bench = Bench::new("trash-twice");
    let file = bench.at("a");
    let client = bench.client();
    fs::write(&file, "first").unwrap();
    client.run(trash(vec![file.clone()]));
    fs::write(&file, "second").unwrap();

    let (_, outcome, _) = client.run(trash(vec![file]));

    assert_eq!(outcome, Outcome::Done);
    let files = home_trash(&bench.root).join("files");
    assert_eq!(fs::read_to_string(files.join("a")).unwrap(), "first");
    assert_eq!(fs::read_to_string(files.join("a.2")).unwrap(), "second");
    assert!(info(&bench, "a.2").contains("Path="));
}

#[test]
fn an_entry_without_an_info_file_keeps_its_name_in_the_trash() {
    let bench = Bench::new("trash-stray");
    let files = home_trash(&bench.root).join("files");
    fs::create_dir_all(&files).unwrap();
    fs::create_dir_all(home_trash(&bench.root).join("info")).unwrap();
    // Another app's entry, its info file not written yet.
    fs::write(files.join("a"), "theirs").unwrap();
    fs::write(bench.at("a"), "ours").unwrap();
    let client = bench.client();

    let (_, outcome, _) = client.run(trash(vec![bench.at("a")]));

    assert_eq!(outcome, Outcome::Done);
    assert_eq!(fs::read_to_string(files.join("a")).unwrap(), "theirs");
    assert_eq!(fs::read_to_string(files.join("a.2")).unwrap(), "ours");
    let info = home_trash(&bench.root).join("info");
    assert!(
        !info.join("a.trashinfo").exists(),
        "its own info file never stays"
    );
}

#[test]
fn a_link_goes_without_what_it_points_to() {
    let bench = Bench::new("trash-link");
    fs::write(bench.at("kept"), "kept").unwrap();
    symlink(bench.at("kept"), bench.at("link")).unwrap();
    let client = bench.client();

    let (_, outcome, _) = client.run(trash(vec![bench.at("link")]));

    assert_eq!(outcome, Outcome::Done);
    assert_eq!(fs::read_to_string(bench.at("kept")).unwrap(), "kept");
    let trashed = home_trash(&bench.root).join("files/link");
    assert!(trashed.symlink_metadata().unwrap().file_type().is_symlink());
}

#[test]
fn a_name_that_is_not_utf8_goes_and_its_path_is_escaped() {
    let bench = Bench::new("trash-bytes");
    let name = OsStr::from_bytes(b"caf\xe9");
    let file = bench.root.join(name);
    fs::write(&file, "x").unwrap();
    let client = bench.client();

    let (_, outcome, _) = client.run(trash(vec![file]));

    assert_eq!(outcome, Outcome::Done);
    assert!(home_trash(&bench.root).join("files").join(name).exists());
    let info_name = OsStr::from_bytes(b"caf\xe9.trashinfo");
    let text = fs::read_to_string(home_trash(&bench.root).join("info").join(info_name)).unwrap();
    assert!(text.contains("/caf%E9\n"), "{text}");
}

#[test]
fn a_bad_path_trashes_nothing() {
    let bench = Bench::new("trash-bad");
    fs::write(bench.at("a"), "a").unwrap();
    let client = bench.client();

    let (_, relative, _) = client.run(trash(vec![bench.at("a"), "a".into()]));
    let (_, gone, _) = client.run(trash(vec![bench.at("a"), bench.at("gone")]));

    assert_eq!(
        relative,
        Outcome::Failed {
            path: "a".into(),
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
    assert!(bench.at("a").exists());
}

/// Waits for job `job`'s conflict, skipping its progress.
fn conflict_of(client: &common::Client, job: JobId) -> nova_files_api::Conflict {
    loop {
        match client.next() {
            Event::JobConflict { job: at, conflict } if at == job => return conflict,
            Event::JobProgress { .. } => {}
            other => panic!("expected a conflict, got {other:?}"),
        }
    }
}

#[test]
fn what_is_in_the_trash_already_asks_and_skip_leaves_it() {
    let bench = Bench::new("trash-in-trash");
    let file = bench.at("a");
    fs::write(&file, "a").unwrap();
    fs::write(bench.at("b"), "b").unwrap();
    let client = bench.client();
    client.run(trash(vec![file]));
    let in_trash = home_trash(&bench.root).join("files/a");

    let job = client.start(trash(vec![in_trash.clone(), bench.at("b")]));
    let conflict = conflict_of(&client, job);
    client.send(Command::AnswerConflict {
        job,
        conflict: conflict.clone(),
        answer: Answer::Skip,
        scope: Scope::ThisOne,
    });

    assert_eq!(conflict.kind, ConflictKind::NoTrash);
    assert_eq!(conflict.source, in_trash);
    let (outcome, _) = loop {
        match client.next() {
            Event::ConflictAnswered { .. } | Event::JobProgress { .. } => {}
            Event::JobFinished {
                outcome, changed, ..
            } => break (outcome, changed),
            other => panic!("expected the job to end, got {other:?}"),
        }
    };
    assert_eq!(outcome, Outcome::Done);
    assert_eq!(fs::read_to_string(&in_trash).unwrap(), "a");
    assert!(home_trash(&bench.root).join("files/b").exists());
}

#[test]
fn a_home_trash_it_may_not_make_asks_and_retry_tries_again() {
    let bench = Bench::new("trash-no-trash");
    let share = bench.root.join("home/.local/share");
    fs::create_dir_all(&share).unwrap();
    // A file where the trash should be: no trash can be made there.
    fs::write(home_trash(&bench.root), "in the way").unwrap();
    fs::write(bench.at("a"), "a").unwrap();
    let client = bench.client();

    let job = client.start(trash(vec![bench.at("a")]));
    let conflict = conflict_of(&client, job);
    fs::remove_file(home_trash(&bench.root)).unwrap();
    client.send(Command::AnswerConflict {
        job,
        conflict: conflict.clone(),
        answer: Answer::Retry,
        scope: Scope::ThisOne,
    });

    assert_eq!(conflict.kind, ConflictKind::NoTrash);
    assert_eq!(
        client.next(),
        Event::ConflictAnswered {
            job,
            answer: Answer::Retry
        }
    );
    let (outcome, _) = client.ended(job);
    assert_eq!(outcome, Outcome::Done);
    assert!(home_trash(&bench.root).join("files/a").exists());
    let mode = fs::metadata(home_trash(&bench.root))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
}

#[test]
fn a_crash_before_the_entry_moved_leaves_no_info_file_and_resumes() {
    let journal_line = |word: &str, path: &Path| format!("{word} {}\n", path.display());
    let entry = |path: &Path| {
        let there = path.symlink_metadata().unwrap();
        format!("{}:{}", there.dev(), there.ino())
    };
    let bench = Bench::with("trash-crash", nova_files_core::IDLE_EXIT, |root| {
        let info = home_trash(root).join("info");
        fs::create_dir_all(&info).unwrap();
        fs::create_dir_all(home_trash(root).join("files")).unwrap();
        fs::write(root.join("a"), "a").unwrap();
        fs::write(info.join("a.trashinfo"), "[Trash Info]\n").unwrap();
        let journals = root.join("home/.local/state/nova-files/jobs");
        fs::create_dir_all(&journals).unwrap();
        let text = format!(
            "nova-files-job 1\ntrash\n{}sources-done\n{}",
            journal_line("source", &root.join("a")),
            journal_line("trash-info", &info.join("a.trashinfo")).replace(
                '\n',
                &format!(
                    " {} {}\n",
                    entry(&info.join("a.trashinfo")),
                    entry(&root.join("a"))
                )
            ),
        );
        fs::write(journals.join("42"), text).unwrap();
    });
    let client = bench.client();
    client.send(Command::WatchJobs);

    let interrupted = client.next();

    let job = JobId(42);
    assert_eq!(
        interrupted,
        Event::JobInterrupted {
            job,
            operation: trash(vec![bench.at("a")])
        }
    );
    let info = home_trash(&bench.root).join("info/a.trashinfo");
    assert!(!info.exists(), "an info file naming nothing goes at once");
    client.send(Command::ResumeJob(job));
    assert_eq!(client.next(), Event::JobResumed(job));
    let (outcome, _) = client.ended(job);
    assert_eq!(outcome, Outcome::Done);
    assert!(!bench.at("a").exists());
    assert!(info.exists() && home_trash(&bench.root).join("files/a").exists());
}

#[test]
fn an_entry_inside_a_folder_trashed_before_it_counts_as_gone() {
    let bench = Bench::new("trash-inside");
    let folder = bench.at("dir");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("file"), "f").unwrap();
    let client = bench.client();

    let (_, outcome, _) = client.run(trash(vec![folder.clone(), folder.join("file")]));

    assert_eq!(outcome, Outcome::Done);
    assert!(home_trash(&bench.root).join("files/dir/file").exists());
}

#[test]
fn a_resumed_trash_job_leaves_what_took_a_trashed_name_since() {
    let bench = Bench::with("trash-resume-new", nova_files_core::IDLE_EXIT, |root| {
        let files = home_trash(root).join("files");
        fs::create_dir_all(&files).unwrap();
        fs::create_dir_all(home_trash(root).join("info")).unwrap();
        fs::write(files.join("a"), "trashed before the crash").unwrap();
        // Made at the old name after the crash.
        fs::write(root.join("a"), "new").unwrap();
        fs::write(root.join("b"), "b").unwrap();
        let journals = root.join("home/.local/state/nova-files/jobs");
        fs::create_dir_all(&journals).unwrap();
        let (a, b) = (root.join("a"), root.join("b"));
        let text = format!(
            "nova-files-job 1\ntrash\nsource {}\nsource {}\nsources-done\ntrashed {} {}\n",
            a.display(),
            b.display(),
            a.display(),
            files.join("a").display()
        );
        fs::write(journals.join("42"), text).unwrap();
    });
    let client = bench.client();
    client.send(Command::WatchJobs);
    let job = JobId(42);
    assert!(matches!(client.next(), Event::JobInterrupted { .. }));

    client.send(Command::ResumeJob(job));

    assert_eq!(client.next(), Event::JobResumed(job));
    assert_eq!(client.ended(job).0, Outcome::Done);
    assert_eq!(fs::read_to_string(bench.at("a")).unwrap(), "new");
    assert!(!bench.at("b").exists());
}

#[test]
fn a_folder_that_holds_the_trash_can_not_go_to_it() {
    let bench = Bench::new("trash-holder");
    let share = bench.root.join("home/.local/share");
    fs::create_dir_all(&share).unwrap();
    let client = bench.client();

    let job = client.start(trash(vec![share.clone()]));
    let conflict = conflict_of(&client, job);

    assert_eq!(conflict.kind, ConflictKind::Unsupported);
    client.send(Command::CancelJob(job));
    assert!(share.is_dir());
}
