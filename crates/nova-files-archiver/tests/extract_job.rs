//! Extract jobs through the real core and helper: where entries land,
//! names already taken, passwords, damage, and undo.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

use std::fs::{self, File};
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use nova_files_api::{
    Answer, Command, Conflict, ConflictKind, Connection, Event, ExtractTo, Failure, JobId, JobTag,
    Operation, Outcome, Scope, Tally,
};
use nova_files_core::{Archiver, Home, IDLE_EXIT, RunningCore};
use zip::write::SimpleFileOptions;

const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);

/// A scratch folder with a core whose home is inside it, and a client.
struct Bench {
    root: PathBuf,
    _core: RunningCore,
    connection: Connection,
    events: Receiver<Event>,
}

impl Bench {
    fn new(test: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("nova-archiver-job-{test}-{}", std::process::id()));
        if root.exists() {
            fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).ok();
            fs::remove_dir_all(&root).ok();
        }
        fs::create_dir_all(root.join("home")).unwrap();
        fs::create_dir_all(root.join("into")).unwrap();
        let home = Home {
            folder: root.join("home"),
            config: root.join("home/.config"),
            state: root.join("home/.local/state"),
            data: root.join("home/.local/share"),
            drive_trashes: false,
        };
        let helper = PathBuf::from(env!("CARGO_BIN_EXE_nova-files-archiver"));
        let core = nova_files_core::spawn_with_archiver(
            IDLE_EXIT,
            Some(home),
            None,
            None,
            Some(Archiver::at(helper)),
        )
        .unwrap();
        let (sink, events) = mpsc::channel();
        let connection = core.connector.connect(sink).unwrap();
        connection.send(Command::WatchJobs).unwrap();
        Self {
            root,
            _core: core,
            connection,
            events,
        }
    }

    fn at(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn start(&self, operation: Operation) -> JobId {
        let tag = JobTag(1);
        self.send(Command::StartJob { tag, operation });
        loop {
            if let Event::JobStarted { tag: got, job, .. } = self.next()
                && got == tag
            {
                return job;
            }
        }
    }

    fn extract(&self, archive: &str, to: ExtractTo) -> JobId {
        self.start(Operation::Extract {
            archive: self.at(archive),
            into: self.at("into"),
            to,
        })
    }

    fn send(&self, command: Command) {
        self.connection.send(command).unwrap();
    }

    fn next(&self) -> Event {
        self.events.recv_timeout(ANSWER_TIMEOUT).unwrap()
    }

    /// The conflict job `job` holds at next.
    fn held(&self, job: JobId) -> Conflict {
        loop {
            match self.next() {
                Event::JobConflict { job: at, conflict } if at == job => return conflict,
                Event::JobFinished { outcome, .. } => panic!("ended {outcome:?} unasked"),
                _ => {}
            }
        }
    }

    /// How job `job` ended, and what answers made of it.
    fn finished(&self, job: JobId) -> (Outcome, Tally) {
        loop {
            match self.next() {
                Event::JobFinished {
                    job: ended,
                    outcome,
                    tally,
                    ..
                } if ended == job => return (outcome, tally),
                Event::JobConflict { conflict, .. } => panic!("held at {conflict:?}"),
                _ => {}
            }
        }
    }

    /// What `into` holds, by name, sorted.
    fn landed(&self) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(self.at("into"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).ok();
    }
}

/// Writes a tar of `files` (path, contents) and `folders` to `archive`.
fn tar(archive: &Path, folders: &[(&str, u32)], files: &[(&str, &str)]) {
    let mut builder = tar::Builder::new(Vec::new());
    for (folder, mode) in folders {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Directory);
        header.set_mode(*mode);
        header.set_size(0);
        builder.append_data(&mut header, folder, &b""[..]).unwrap();
    }
    for (path, contents) in files {
        let mut header = tar::Header::new_gnu();
        header.set_mode(0o644);
        header.set_size(contents.len() as u64);
        builder
            .append_data(&mut header, path, contents.as_bytes())
            .unwrap();
    }
    fs::write(archive, builder.into_inner().unwrap()).unwrap();
}

#[test]
fn one_folder_at_the_top_goes_straight_in_and_undo_takes_it_back() {
    let bench = Bench::new("one");
    tar(
        &bench.at("docs.tar"),
        &[("docs/", 0o755)],
        &[("docs/a.txt", "a")],
    );
    let job = bench.extract("docs.tar", ExtractTo::Here);
    assert_eq!(bench.finished(job), (Outcome::Done, Tally::default()));
    assert_eq!(bench.landed(), ["docs"]);
    assert_eq!(fs::read(bench.at("into/docs/a.txt")).unwrap(), b"a");

    let undo = bench.start(Operation::Undo { job });
    assert_eq!(bench.finished(undo).0, Outcome::Done);
    assert_eq!(bench.landed(), Vec::<String>::new());
}

#[test]
fn several_at_the_top_go_into_a_folder_named_after_the_archive() {
    let bench = Bench::new("several");
    tar(
        &bench.at("src-1.2.tar"),
        &[],
        &[("a.txt", "a"), ("b.txt", "b")],
    );
    let job = bench.extract("src-1.2.tar", ExtractTo::Here);
    assert_eq!(bench.finished(job).0, Outcome::Done);
    assert_eq!(bench.landed(), ["src-1.2"]);
    assert_eq!(fs::read(bench.at("into/src-1.2/b.txt")).unwrap(), b"b");
}

#[test]
fn inside_puts_everything_straight_in() {
    let bench = Bench::new("inside");
    tar(&bench.at("a.tar"), &[], &[("a.txt", "a"), ("b.txt", "b")]);
    let job = bench.extract("a.tar", ExtractTo::Inside);
    assert_eq!(bench.finished(job).0, Outcome::Done);
    assert_eq!(bench.landed(), ["a.txt", "b.txt"]);
}

#[test]
fn a_name_already_taken_is_asked_about() {
    let bench = Bench::new("taken");
    tar(
        &bench.at("docs.tar"),
        &[("docs/", 0o755)],
        &[("docs/a.txt", "new")],
    );
    fs::create_dir(bench.at("into/docs")).unwrap();
    let job = bench.extract("docs.tar", ExtractTo::Here);
    let conflict = bench.held(job);
    assert_eq!(conflict.kind, ConflictKind::FolderExists);
    assert_eq!(conflict.target, bench.at("into/docs"));
    bench.send(Command::AnswerConflict {
        job,
        conflict,
        answer: Answer::KeepBoth,
        scope: Scope::ThisOne,
    });
    assert_eq!(bench.finished(job).0, Outcome::Done);
    assert_eq!(bench.landed(), ["docs", "docs (2)"]);
    assert_eq!(fs::read(bench.at("into/docs (2)/a.txt")).unwrap(), b"new");
}

/// A zip whose one file is locked with `password`.
fn locked_zip(archive: &Path, password: &str) {
    let mut zip = zip::ZipWriter::new(File::create(archive).unwrap());
    zip.start_file(
        "secret.txt",
        SimpleFileOptions::default().with_aes_encryption(zip::AesMode::Aes256, password),
    )
    .unwrap();
    zip.write_all(b"hidden").unwrap();
    zip.finish().unwrap();
}

#[test]
fn a_locked_archive_asks_for_its_password_until_it_is_right() {
    let bench = Bench::new("password");
    locked_zip(&bench.at("secret.zip"), "pw");
    let job = bench.extract("secret.zip", ExtractTo::Here);
    let needed = bench.held(job);
    assert_eq!(needed.kind, ConflictKind::PasswordNeeded);
    assert_eq!(needed.source, bench.at("secret.zip"));
    bench.send(Command::GivePassword {
        job,
        conflict: needed,
        password: "wrong".to_owned(),
    });
    let wrong = bench.held(job);
    assert_eq!(wrong.kind, ConflictKind::PasswordWrong);
    bench.send(Command::GivePassword {
        job,
        conflict: wrong,
        password: "pw".to_owned(),
    });
    assert_eq!(bench.finished(job).0, Outcome::Done);
    assert_eq!(bench.landed(), ["secret.txt"]);
    assert_eq!(fs::read(bench.at("into/secret.txt")).unwrap(), b"hidden");
}

#[test]
fn skipping_the_password_cancels_and_leaves_nothing() {
    let bench = Bench::new("skip");
    locked_zip(&bench.at("secret.zip"), "pw");
    let job = bench.extract("secret.zip", ExtractTo::Here);
    let needed = bench.held(job);
    // A password given to another kind of conflict is never taken.
    bench.send(Command::GivePassword {
        job,
        conflict: Conflict {
            kind: ConflictKind::NoSpace,
            ..needed.clone()
        },
        password: "pw".to_owned(),
    });
    bench.send(Command::AnswerConflict {
        job,
        conflict: needed,
        answer: Answer::Skip,
        scope: Scope::ThisOne,
    });
    let (outcome, tally) = bench.finished(job);
    assert_eq!(outcome, Outcome::Cancelled);
    assert_eq!(tally, Tally::default());
    assert_eq!(bench.landed(), Vec::<String>::new());
}

#[test]
fn damage_fails_the_job_and_leaves_nothing() {
    let bench = Bench::new("damage");
    let mut bytes = Vec::new();
    tar(&bench.at("a.tar"), &[], &[("a.txt", "a")]);
    let mut gzip = flate2::write::GzEncoder::new(&mut bytes, flate2::Compression::default());
    gzip.write_all(&fs::read(bench.at("a.tar")).unwrap())
        .unwrap();
    gzip.finish().unwrap();
    bytes.truncate(bytes.len() / 2);
    fs::write(bench.at("a.tar.gz"), bytes).unwrap();
    let job = bench.extract("a.tar.gz", ExtractTo::Here);
    assert_eq!(
        bench.finished(job).0,
        Outcome::Failed {
            path: bench.at("a.tar.gz"),
            reason: Failure::Damaged,
        }
    );
    assert_eq!(bench.landed(), Vec::<String>::new());
}

#[test]
fn a_skipped_folder_with_a_read_only_one_inside_leaves_nothing_behind() {
    let bench = Bench::new("skip-read-only");
    tar(
        &bench.at("ro.tar"),
        &[("ro/", 0o755), ("ro/inner/", 0o555)],
        &[("ro/inner/a.txt", "a")],
    );
    fs::create_dir(bench.at("into/ro")).unwrap();
    let job = bench.extract("ro.tar", ExtractTo::Here);
    let conflict = bench.held(job);
    bench.send(Command::AnswerConflict {
        job,
        conflict,
        answer: Answer::Skip,
        scope: Scope::ThisOne,
    });
    assert_eq!(bench.finished(job).0, Outcome::Done);
    assert_eq!(bench.landed(), ["ro"]);
}

#[test]
fn a_relative_archive_path_is_an_invalid_name() {
    let bench = Bench::new("relative");
    let job = bench.start(Operation::Extract {
        archive: "a.tar".into(),
        into: bench.at("into"),
        to: ExtractTo::Here,
    });
    assert_eq!(
        bench.finished(job).0,
        Outcome::Failed {
            path: "a.tar".into(),
            reason: Failure::InvalidName,
        }
    );
}

#[test]
fn a_read_only_folder_moves_into_place_and_stays_read_only() {
    let bench = Bench::new("read-only");
    tar(&bench.at("ro.tar"), &[("ro/", 0o555)], &[("ro/a.txt", "a")]);
    let job = bench.extract("ro.tar", ExtractTo::Here);
    assert_eq!(bench.finished(job).0, Outcome::Done);
    let ro = bench.at("into/ro");
    assert_eq!(
        fs::metadata(&ro).unwrap().permissions().mode() & 0o777,
        0o555
    );
    fs::set_permissions(&ro, fs::Permissions::from_mode(0o755)).unwrap();
}
