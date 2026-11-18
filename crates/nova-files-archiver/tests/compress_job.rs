//! Compress jobs through the real core and helper: the archive takes its
//! name or the next free one, a password locks it, and undo takes it back.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

mod common;

use std::fs;
use std::path::PathBuf;

use common::Bench;
use nova_files_api::{
    Answer, Command, ConflictKind, Failure, Operation, Outcome, Packing, Scope, Tally,
};
use nova_files_core::{ArchiveKind, Archiver};

fn compress(bench: &Bench, name: &str, locked: bool) -> nova_files_api::JobId {
    bench.start(Operation::Compress {
        sources: vec![bench.at("docs"), bench.at("notes.txt")],
        into: bench.at("into"),
        name: name.into(),
        packing: Packing::ZipDeflate,
        level: 5,
        locked,
    })
}

fn with_sources(test: &str) -> Bench {
    let bench = Bench::new(test);
    fs::create_dir(bench.at("docs")).unwrap();
    fs::write(bench.at("docs/a.txt"), b"a").unwrap();
    fs::write(bench.at("notes.txt"), b"n").unwrap();
    bench
}

fn archiver() -> Archiver {
    Archiver::at(PathBuf::from(env!("CARGO_BIN_EXE_nova-files-archiver")))
}

#[test]
fn makes_the_archive_and_undo_takes_it_back() {
    let bench = with_sources("compress");
    let job = compress(&bench, "docs.zip", false);
    assert_eq!(bench.finished(job), (Outcome::Done, Tally::default()));
    assert_eq!(bench.landed(), ["docs.zip"]);
    let entries = archiver().list(&bench.at("into/docs.zip")).unwrap();
    assert_eq!(entries.len(), 3);

    let undo = bench.start(Operation::Undo { job });
    assert_eq!(bench.finished(undo).0, Outcome::Done);
    assert_eq!(bench.landed(), Vec::<String>::new());
    assert!(bench.at("docs/a.txt").exists());
}

#[test]
fn a_taken_name_gives_the_next_free_one() {
    let bench = with_sources("compress-taken");
    fs::write(bench.at("into/docs.zip"), b"mine").unwrap();
    let job = compress(&bench, "docs.zip", false);
    assert_eq!(bench.finished(job).0, Outcome::Done);
    assert_eq!(bench.landed(), ["docs (2).zip", "docs.zip"]);
    assert_eq!(fs::read(bench.at("into/docs.zip")).unwrap(), b"mine");
}

#[test]
fn locked_asks_for_the_password_first() {
    let bench = with_sources("compress-locked");
    let job = compress(&bench, "docs.zip", true);
    let needed = bench.held(job);
    assert_eq!(needed.kind, ConflictKind::PasswordNeeded);
    bench.send(Command::GivePassword {
        job,
        conflict: needed,
        password: String::new(),
    });
    let again = bench.held(job);
    assert_eq!(again.kind, ConflictKind::PasswordNeeded);
    bench.send(Command::GivePassword {
        job,
        conflict: again,
        password: "pw".to_owned(),
    });
    assert_eq!(bench.finished(job).0, Outcome::Done);
    let entries = archiver().list(&bench.at("into/docs.zip")).unwrap();
    assert!(
        entries
            .iter()
            .filter(|entry| entry.kind == ArchiveKind::File)
            .all(|entry| entry.locked)
    );
}

#[test]
fn a_bad_name_fails_and_leaves_nothing() {
    let bench = with_sources("compress-bad");
    let job = compress(&bench, "a/b.zip", false);
    assert_eq!(
        bench.finished(job).0,
        Outcome::Failed {
            path: bench.at("into/a/b.zip"),
            reason: Failure::InvalidName,
        }
    );
    assert_eq!(bench.landed(), Vec::<String>::new());
}

#[test]
fn relative_paths_are_invalid_names() {
    let bench = with_sources("compress-relative");
    let job = bench.start(Operation::Compress {
        sources: vec![bench.at("docs")],
        into: "into".into(),
        name: "a.zip".into(),
        packing: Packing::ZipDeflate,
        level: 5,
        locked: false,
    });
    assert_eq!(
        bench.finished(job).0,
        Outcome::Failed {
            path: "into/a.zip".into(),
            reason: Failure::InvalidName,
        }
    );
    let job = bench.start(Operation::Compress {
        sources: vec!["docs".into()],
        into: bench.at("into"),
        name: "a.zip".into(),
        packing: Packing::ZipDeflate,
        level: 5,
        locked: false,
    });
    assert_eq!(
        bench.finished(job).0,
        Outcome::Failed {
            path: "docs".into(),
            reason: Failure::InvalidName,
        }
    );
    assert_eq!(bench.landed(), Vec::<String>::new());
}

#[test]
fn skipping_the_password_cancels_and_makes_nothing() {
    let bench = with_sources("compress-skip");
    let job = compress(&bench, "docs.zip", true);
    let needed = bench.held(job);
    bench.send(Command::AnswerConflict {
        job,
        conflict: needed,
        answer: Answer::Skip,
        scope: Scope::ThisOne,
    });
    assert_eq!(bench.finished(job), (Outcome::Cancelled, Tally::default()));
    assert_eq!(bench.landed(), Vec::<String>::new());
}
