#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

mod common;

use std::fs::{self, File};
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

use common::Bench;
use nova_files_api::{
    Answer, Command, Conflict, ConflictKind, ExtractTo, Failure, JobId, Operation, Outcome, Scope,
    Tally,
};
use zip::write::SimpleFileOptions;

fn extract(bench: &Bench, archive: &str, to: ExtractTo) -> JobId {
    bench.start(Operation::Extract {
        archive: bench.at(archive),
        into: bench.at("into"),
        to,
    })
}

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
    let job = extract(&bench, "docs.tar", ExtractTo::Here);
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
    let job = extract(&bench, "src-1.2.tar", ExtractTo::Here);
    assert_eq!(bench.finished(job).0, Outcome::Done);
    assert_eq!(bench.landed(), ["src-1.2"]);
    assert_eq!(fs::read(bench.at("into/src-1.2/b.txt")).unwrap(), b"b");
}

#[test]
fn inside_puts_everything_straight_in() {
    let bench = Bench::new("inside");
    tar(&bench.at("a.tar"), &[], &[("a.txt", "a"), ("b.txt", "b")]);
    let job = extract(&bench, "a.tar", ExtractTo::Inside);
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
    let job = extract(&bench, "docs.tar", ExtractTo::Here);
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
    let job = extract(&bench, "secret.zip", ExtractTo::Here);
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
    let job = extract(&bench, "secret.zip", ExtractTo::Here);
    let needed = bench.held(job);
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
    let job = extract(&bench, "a.tar.gz", ExtractTo::Here);
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
    let job = extract(&bench, "ro.tar", ExtractTo::Here);
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
    let job = extract(&bench, "ro.tar", ExtractTo::Here);
    assert_eq!(bench.finished(job).0, Outcome::Done);
    let ro = bench.at("into/ro");
    assert_eq!(
        fs::metadata(&ro).unwrap().permissions().mode() & 0o777,
        0o555
    );
    fs::set_permissions(&ro, fs::Permissions::from_mode(0o755)).unwrap();
}
