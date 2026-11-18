//! 7z through the installed 7-Zip, run by the real helper: made, listed
//! and unpacked whole, locked with its names hidden, stopped, and turned
//! away without 7-Zip. These tests need 7-Zip (`7zz` or `7z`) on `PATH`.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::Bench;
use nova_files_api::{Command, ConflictKind, Event, ExtractTo, Operation, Outcome, Packing, Tally};
use nova_files_core::{ArchiveFailure, ArchiveKind, Archiver, Extracting, Flow, Recipe};

/// Entries whose listing outgrows the helper's output buffer.
const MANY: u32 = 500;
/// More asks with nothing new than a test archive takes.
const MOST_WAITS: u32 = 100;
/// Well under the 10 s a helper gets to exit once its output ends.
const QUICK: Duration = Duration::from_secs(5);

fn helper() -> Archiver {
    Archiver::at(PathBuf::from(env!("CARGO_BIN_EXE_nova-files-archiver")))
}

fn archiver() -> Archiver {
    let seven = nova_files_core::seven_on(std::env::var_os("PATH").as_deref())
        .expect("these tests need 7-Zip (7zz or 7z) on PATH");
    helper().with_seven(Some(seven))
}

/// Goes on, counting entries, but stops a run asked this many times with
/// nothing new.
fn go(entries: &mut u64) -> impl FnMut(Extracting) -> Flow + '_ {
    let mut waits = 0;
    move |extracting| {
        match extracting {
            Extracting::Entry(_) => *entries += 1,
            Extracting::Waiting => waits += 1,
            Extracting::Held => {}
        }
        if waits < MOST_WAITS {
            Flow::Go
        } else {
            Flow::Stop
        }
    }
}

/// A fresh scratch folder holding `docs/` (a file, a folder inside, a
/// link) and `notes.txt`.
fn sources(name: &str) -> PathBuf {
    let folder =
        std::env::temp_dir().join(format!("nova-archiver-seven-{name}-{}", std::process::id()));
    fs::remove_dir_all(&folder).ok();
    fs::create_dir_all(folder.join("docs/inner")).unwrap();
    fs::create_dir_all(folder.join("into")).unwrap();
    for (path, contents) in [
        ("docs/a.txt", "hello"),
        ("docs/inner/b.txt", "deep"),
        ("notes.txt", "notes"),
    ] {
        fs::write(folder.join(path), contents).unwrap();
    }
    fs::set_permissions(folder.join("docs/a.txt"), fs::Permissions::from_mode(0o640)).unwrap();
    std::os::unix::fs::symlink("a.txt", folder.join("docs/to-a")).unwrap();
    folder
}

fn make(folder: &Path, password: Option<&str>) -> PathBuf {
    let archive = folder.join("made.7z");
    let sources = [folder.join("docs"), folder.join("notes.txt")];
    let recipe = Recipe {
        packing: Packing::SevenZip,
        level: 5,
        password,
    };
    let mut entries = 0;
    archiver()
        .compress(&sources, &archive, &recipe, go(&mut entries))
        .unwrap();
    assert_eq!(entries, 6);
    archive
}

fn kinds(archive: &Path, password: Option<&str>) -> Vec<(String, ArchiveKind, u64, u32, bool)> {
    let mut entries: Vec<_> = archiver()
        .list_locked(archive, password)
        .unwrap()
        .into_iter()
        .map(|entry| {
            (
                entry.path.to_string_lossy().into_owned(),
                entry.kind,
                entry.size,
                entry.mode,
                entry.locked,
            )
        })
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

#[test]
fn a_7z_is_made_listed_and_unpacked_whole() {
    let folder = sources("whole");
    let archive = make(&folder, None);
    assert_eq!(
        kinds(&archive, None),
        [
            ("docs".to_owned(), ArchiveKind::Folder, 0, 0o755, false),
            ("docs/a.txt".to_owned(), ArchiveKind::File, 5, 0o640, false),
            (
                "docs/inner".to_owned(),
                ArchiveKind::Folder,
                0,
                0o755,
                false
            ),
            (
                "docs/inner/b.txt".to_owned(),
                ArchiveKind::File,
                4,
                0o644,
                false
            ),
            ("docs/to-a".to_owned(), ArchiveKind::Link, 5, 0o777, false),
            ("notes.txt".to_owned(), ArchiveKind::File, 5, 0o644, false),
        ]
    );
    let into = folder.join("into");
    let mut entries = 0;
    archiver()
        .extract(&archive, &into, None, go(&mut entries))
        .unwrap();
    assert_eq!(entries, 6);
    assert_eq!(fs::read(into.join("docs/a.txt")).unwrap(), b"hello");
    assert_eq!(fs::read(into.join("docs/inner/b.txt")).unwrap(), b"deep");
    assert_eq!(
        fs::read_link(into.join("docs/to-a")).unwrap(),
        Path::new("a.txt")
    );
    assert_eq!(
        fs::metadata(into.join("docs/a.txt"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
    fs::remove_dir_all(&folder).ok();
}

#[test]
fn a_locked_7z_hides_its_names_until_the_password_is_right() {
    let folder = sources("locked");
    let archive = make(&folder, Some("pw"));
    assert_eq!(
        archiver().list(&archive),
        Err(ArchiveFailure::NeedsPassword)
    );
    assert_eq!(
        archiver().list_locked(&archive, Some("nope")),
        Err(ArchiveFailure::WrongPassword)
    );
    let listed = kinds(&archive, Some("pw"));
    assert_eq!(listed.len(), 6);
    assert!(
        listed
            .iter()
            .filter(|entry| entry.1 == ArchiveKind::File)
            .all(|entry| entry.4)
    );
    let into = folder.join("into");
    let mut entries = 0;
    assert_eq!(
        archiver().extract(&archive, &into, None, go(&mut entries)),
        Err(ArchiveFailure::NeedsPassword)
    );
    assert_eq!(
        archiver().extract(&archive, &into, Some("nope"), go(&mut entries)),
        Err(ArchiveFailure::WrongPassword)
    );
    archiver()
        .extract(&archive, &into, Some("pw"), go(&mut entries))
        .unwrap();
    assert_eq!(fs::read(into.join("notes.txt")).unwrap(), b"notes");
    fs::remove_dir_all(&folder).ok();
}

#[test]
fn without_7zip_a_7z_is_unsupported() {
    let folder = sources("without");
    let archive = make(&folder, None);
    let into = folder.join("into");
    let mut entries = 0;
    assert_eq!(helper().list(&archive), Err(ArchiveFailure::Unsupported));
    assert_eq!(
        helper().extract(&archive, &into, None, go(&mut entries)),
        Err(ArchiveFailure::Unsupported)
    );
    let recipe = Recipe {
        packing: Packing::SevenZip,
        level: 5,
        password: None,
    };
    assert_eq!(
        helper().compress(
            &[folder.join("docs")],
            &folder.join("b.7z"),
            &recipe,
            go(&mut entries)
        ),
        Err(ArchiveFailure::Unsupported)
    );
    assert!(!folder.join("b.7z").exists());
    assert_eq!(entries, 0);
    fs::remove_dir_all(&folder).ok();
}

#[test]
fn a_damaged_7z_is_broken_and_other_files_unsupported() {
    let folder = sources("damage");
    let archive = make(&folder, None);
    let mut bytes = fs::read(&archive).unwrap();
    let cut = bytes.len() / 2;
    bytes.truncate(cut);
    fs::write(folder.join("cut.7z"), &bytes).unwrap();
    fs::write(folder.join("mark.7z"), b"7z\xbc\xaf\x27\x1cnothing more").unwrap();
    assert_eq!(
        archiver().list(&folder.join("cut.7z")),
        Err(ArchiveFailure::Broken)
    );
    assert_eq!(
        archiver().list(&folder.join("mark.7z")),
        Err(ArchiveFailure::Broken)
    );
    assert_eq!(
        archiver().list(&folder.join("notes.txt")),
        Err(ArchiveFailure::Unsupported)
    );
    let mut entries = 0;
    assert_eq!(
        archiver().extract(
            &folder.join("cut.7z"),
            &folder.join("into"),
            None,
            go(&mut entries)
        ),
        Err(ArchiveFailure::Broken)
    );
    fs::remove_dir_all(&folder).ok();
}

#[test]
fn stopping_a_7z_run_ends_it_at_once() {
    let folder = sources("stop");
    let archive = make(&folder, None);
    let started = Instant::now();
    let stopped = archiver().extract(&archive, &folder.join("into"), None, |_| Flow::Stop);
    assert_eq!(stopped, Err(ArchiveFailure::Cancelled));
    assert!(started.elapsed() < QUICK);
    fs::remove_dir_all(&folder).ok();
}

#[test]
fn a_job_asks_for_the_password_a_locked_7z_needs_to_list() {
    let bench = Bench::new("seven-job");
    fs::create_dir(bench.at("docs")).unwrap();
    fs::write(bench.at("docs/a.txt"), b"a").unwrap();
    let job = bench.start(Operation::Compress {
        sources: vec![bench.at("docs")],
        into: bench.at("into"),
        name: "docs.7z".into(),
        packing: Packing::SevenZip,
        level: 5,
        locked: true,
    });
    let needed = bench.held(job);
    bench.send(Command::GivePassword {
        job,
        conflict: needed,
        password: "pw".to_owned(),
    });
    assert_eq!(bench.finished(job), (Outcome::Done, Tally::default()));
    fs::rename(bench.at("into/docs.7z"), bench.at("docs.7z")).unwrap();
    let job = bench.start(Operation::Extract {
        archive: bench.at("docs.7z"),
        into: bench.at("into"),
        to: ExtractTo::Here,
    });
    let needed = bench.held(job);
    assert_eq!(needed.kind, ConflictKind::PasswordNeeded);
    bench.send(Command::GivePassword {
        job,
        conflict: needed,
        password: "nope".to_owned(),
    });
    let wrong = bench.held(job);
    assert_eq!(wrong.kind, ConflictKind::PasswordWrong);
    bench.send(Command::GivePassword {
        job,
        conflict: wrong,
        password: "pw".to_owned(),
    });
    let mut planned = None;
    loop {
        match bench.next() {
            Event::JobProgress { progress, .. } => {
                planned = planned.or(Some(progress.total_bytes));
            }
            Event::JobFinished { outcome, .. } => {
                assert_eq!(outcome, Outcome::Done);
                break;
            }
            _ => {}
        }
    }
    assert_eq!(
        planned,
        Some(1),
        "the plan counts the bytes the password showed"
    );
    assert_eq!(bench.landed(), ["docs"]);
    assert_eq!(fs::read(bench.at("into/docs/a.txt")).unwrap(), b"a");
}

#[test]
fn a_7z_gives_only_the_picked_entries() {
    let folder = sources("picked");
    let archive = make(&folder, None);
    let into = folder.join("into");
    let mut entries = 0;
    let picked = [PathBuf::from("docs/inner"), PathBuf::from("notes.txt")];
    archiver()
        .extract_picked((&archive, &picked), &into, None, go(&mut entries))
        .unwrap();
    assert_eq!(fs::read(into.join("docs/inner/b.txt")).unwrap(), b"deep");
    assert_eq!(fs::read(into.join("notes.txt")).unwrap(), b"notes");
    assert!(!into.join("docs/a.txt").exists());
    fs::remove_dir_all(&folder).ok();
}

#[test]
fn a_listing_no_one_reads_is_a_usage_error() {
    let folder = sources("unheard");
    let many = folder.join("many");
    fs::create_dir(&many).unwrap();
    for number in 0..MANY {
        fs::write(
            many.join(format!("a-long-file-name-for-a-long-listing-{number}")),
            b"",
        )
        .unwrap();
    }
    let archive = folder.join("many.7z");
    let recipe = Recipe {
        packing: Packing::SevenZip,
        level: 1,
        password: None,
    };
    let mut entries = 0;
    archiver()
        .compress(&[many], &archive, &recipe, go(&mut entries))
        .unwrap();
    let seven = nova_files_core::seven_on(std::env::var_os("PATH").as_deref()).unwrap();
    let full = fs::OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .unwrap();
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_nova-files-archiver"))
        .arg("--seven")
        .arg(seven)
        .arg("list")
        .arg(&archive)
        .stdin(std::process::Stdio::null())
        .stdout(full)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(1));
    fs::remove_dir_all(&folder).ok();
}

#[test]
fn a_name_with_wildcards_picks_only_itself() {
    let folder = sources("wildcards");
    for name in ["a?.txt", "ab.txt"] {
        fs::write(folder.join(name), name).unwrap();
    }
    let archive = folder.join("wild.7z");
    let recipe = Recipe {
        packing: Packing::SevenZip,
        level: 1,
        password: None,
    };
    let mut entries = 0;
    archiver()
        .compress(
            &[folder.join("a?.txt")],
            &archive,
            &recipe,
            go(&mut entries),
        )
        .unwrap();
    let names: Vec<_> = kinds(&archive, None)
        .into_iter()
        .map(|kind| kind.0)
        .collect();
    assert_eq!(names, ["a?.txt"]);
    fs::remove_dir_all(&folder).ok();
}

/// The shell on PATH, by its real path: the sandbox lets 7-Zip run only
/// programs under its own folder and the system folders.
fn shell_on_path() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|folder| folder.join("sh"))
        .find_map(|shell| fs::canonicalize(shell).ok())
        .unwrap()
}

#[test]
fn seven_zip_may_read_the_kernels_random_numbers() {
    let folder = sources("random");
    let program = folder.join("7zz");
    let shell = shell_on_path();
    fs::write(
        &program,
        format!(
            "#!{}\nread -r line < /dev/urandom || exit 2\n",
            shell.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    let archive = folder.join("any.7z");
    fs::write(&archive, b"7z\xbc\xaf\x27\x1c").unwrap();
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_nova-files-archiver"))
        .arg("--seven")
        .arg(&program)
        .arg("list")
        .arg(&archive)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert_eq!(status.status.code(), Some(0));
    fs::remove_dir_all(&folder).ok();
}

#[test]
fn a_picked_name_with_wildcards_is_taken_as_it_is() {
    let folder = sources("picked-wild");
    for name in ["report?.txt", "report1.txt"] {
        fs::write(folder.join(name), name).unwrap();
    }
    let archive = folder.join("reports.7z");
    let recipe = Recipe {
        packing: Packing::SevenZip,
        level: 1,
        password: None,
    };
    let mut entries = 0;
    let sources = [folder.join("report?.txt"), folder.join("report1.txt")];
    archiver()
        .compress(&sources, &archive, &recipe, go(&mut entries))
        .unwrap();
    let into = folder.join("into");
    archiver()
        .extract_picked(
            (&archive, &[PathBuf::from("report?.txt")]),
            &into,
            None,
            go(&mut entries),
        )
        .unwrap();
    let mut landed: Vec<_> = fs::read_dir(&into)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    landed.sort();
    assert_eq!(landed, ["report?.txt"]);
    fs::remove_dir_all(&folder).ok();
}
