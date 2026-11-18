//! The core makes archives through the real helper, in every packing,
//! and reads them back the same way.

#![expect(clippy::unwrap_used, reason = "test setup should fail loudly")]

use std::fs;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use nova_files_api::Packing;
use nova_files_core::{ArchiveFailure, ArchiveKind, Archiver, Extracting, Flow, Recipe};

/// More asks with nothing new than a test archive takes.
const MOST_WAITS: u32 = 100;

/// Goes on, but stops a run asked this many times with nothing new.
fn go() -> impl FnMut(Extracting) -> Flow {
    let mut waits = 0;
    move |extracting| {
        if extracting == Extracting::Waiting {
            waits += 1;
        }
        if waits < MOST_WAITS {
            Flow::Go
        } else {
            Flow::Stop
        }
    }
}

fn archiver() -> Archiver {
    Archiver::at(PathBuf::from(env!("CARGO_BIN_EXE_nova-files-archiver")))
}

/// A fresh scratch folder holding `docs/` (a file, a folder inside, a
/// link) and `notes.txt`.
fn sources(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!(
        "nova-archiver-compress-{name}-{}",
        std::process::id()
    ));
    fs::remove_dir_all(&folder).ok();
    fs::create_dir_all(folder.join("docs/inner")).unwrap();
    fs::write(folder.join("docs/a.txt"), b"hello").unwrap();
    fs::set_permissions(folder.join("docs/a.txt"), fs::Permissions::from_mode(0o640)).unwrap();
    fs::write(folder.join("docs/inner/b.txt"), b"deep").unwrap();
    std::os::unix::fs::symlink("a.txt", folder.join("docs/to-a")).unwrap();
    fs::write(folder.join("notes.txt"), b"notes").unwrap();
    fs::set_permissions(folder.join("docs"), fs::Permissions::from_mode(0o750)).unwrap();
    folder
}

fn make(folder: &Path, archive: &str, recipe: &Recipe<'_>) -> Result<PathBuf, ArchiveFailure> {
    let archive = folder.join(archive);
    let sources = [folder.join("docs"), folder.join("notes.txt")];
    archiver()
        .compress(&sources, &archive, recipe, go())
        .map(|()| archive)
}

const fn recipe(packing: Packing) -> Recipe<'static> {
    Recipe {
        packing,
        level: 5,
        password: None,
    }
}

#[test]
fn every_packing_reads_back_whole() {
    let folder = sources("every");
    let modified = fs::metadata(folder.join("docs/a.txt")).unwrap().mtime();
    for (packing, name) in [
        (Packing::ZipDeflate, "a.zip"),
        (Packing::ZipZstd, "z.zip"),
        (Packing::TarZstd, "a.tar.zst"),
        (Packing::TarGzip, "a.tar.gz"),
        (Packing::TarXz, "a.tar.xz"),
    ] {
        let archive = make(&folder, name, &recipe(packing)).unwrap();
        let entries = archiver().list(&archive).unwrap();
        let paths: Vec<_> = entries.iter().map(|entry| entry.path.as_path()).collect();
        assert_eq!(
            paths,
            [
                Path::new("docs"),
                Path::new("docs/a.txt"),
                Path::new("docs/inner"),
                Path::new("docs/inner/b.txt"),
                Path::new("docs/to-a"),
                Path::new("notes.txt"),
            ],
            "{name}"
        );
        assert_eq!(entries[0].kind, ArchiveKind::Folder, "{name}");
        assert_eq!(entries[0].mode, 0o750, "{name}");
        assert_eq!(entries[1].mode, 0o640, "{name}");
        assert_eq!(entries[1].modified, modified, "{name}");
        assert_eq!(entries[4].kind, ArchiveKind::Link, "{name}");
        assert_eq!(entries[4].link, Path::new("a.txt"), "{name}");

        let into = folder.join(format!("out-{name}"));
        fs::create_dir(&into).unwrap();
        archiver().extract(&archive, &into, None, go()).unwrap();
        assert_eq!(fs::read(into.join("docs/inner/b.txt")).unwrap(), b"deep");
        assert_eq!(fs::read(into.join("notes.txt")).unwrap(), b"notes");
    }
    fs::remove_dir_all(&folder).ok();
}

#[test]
fn a_password_locks_each_file_of_a_zip() {
    let folder = sources("password");
    let locked = Recipe {
        password: Some("pw"),
        ..recipe(Packing::ZipDeflate)
    };
    let archive = make(&folder, "a.zip", &locked).unwrap();
    let entries = archiver().list(&archive).unwrap();
    let files: Vec<_> = entries
        .iter()
        .filter(|entry| entry.kind == ArchiveKind::File)
        .collect();
    assert!(files.iter().all(|entry| entry.locked));
    assert!(
        entries
            .iter()
            .filter(|entry| entry.kind != ArchiveKind::File)
            .all(|entry| !entry.locked)
    );
    let into = folder.join("out");
    fs::create_dir(&into).unwrap();
    assert_eq!(
        archiver().extract(&archive, &into, None, go()),
        Err(ArchiveFailure::NeedsPassword)
    );
    archiver()
        .extract(&archive, &into, Some("pw"), go())
        .unwrap();
    assert_eq!(fs::read(into.join("docs/a.txt")).unwrap(), b"hello");
    fs::remove_dir_all(&folder).ok();
}

#[test]
fn levels_past_either_end_are_the_ends() {
    let folder = sources("levels");
    for level in [0, 1, 9, 200] {
        let recipe = Recipe {
            level,
            ..recipe(Packing::TarZstd)
        };
        let archive = make(&folder, &format!("{level}.tar.zst"), &recipe).unwrap();
        assert_eq!(archiver().list(&archive).unwrap().len(), 6);
    }
    fs::remove_dir_all(&folder).ok();
}

#[test]
fn it_never_writes_over_a_file_or_reads_what_is_gone() {
    let folder = sources("refused");
    fs::write(folder.join("taken.zip"), b"mine").unwrap();
    assert_eq!(
        make(&folder, "taken.zip", &recipe(Packing::ZipDeflate)),
        Err(ArchiveFailure::NotWritten)
    );
    assert_eq!(fs::read(folder.join("taken.zip")).unwrap(), b"mine");
    let gone = [folder.join("gone")];
    assert_eq!(
        archiver().compress(
            &gone,
            &folder.join("b.zip"),
            &recipe(Packing::ZipDeflate),
            go()
        ),
        Err(ArchiveFailure::Gone)
    );
    fs::remove_dir_all(&folder).ok();
}

#[test]
fn an_unknown_packing_is_a_usage_error() {
    let folder = sources("usage");
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_nova-files-archiver"))
        .args(["compress", "rar", "5"])
        .arg(folder.join("a.rar"))
        .arg(folder.join("notes.txt"))
        .output()
        .unwrap();
    assert_eq!(status.status.code(), Some(1));
    assert!(!folder.join("a.rar").exists());
    fs::remove_dir_all(&folder).ok();
}

#[test]
fn a_compress_without_sources_or_of_another_ask_is_a_usage_error() {
    let folder = sources("usage-shape");
    for args in [
        ["compress", "zip-deflate", "5", "a.zip", ""],
        ["show", "zip-deflate", "5", "a.zip", "notes.txt"],
    ] {
        let args = args.iter().filter(|arg| !arg.is_empty());
        let status = std::process::Command::new(env!("CARGO_BIN_EXE_nova-files-archiver"))
            .current_dir(&folder)
            .args(args)
            .output()
            .unwrap();
        assert_eq!(status.status.code(), Some(1));
        assert!(!folder.join("a.zip").exists());
    }
    fs::remove_dir_all(&folder).ok();
}

/// Text that compresses better the harder it is tried.
fn wordy(folder: &Path) -> PathBuf {
    let words = [
        "archive", "folder", "nova", "files", "zip", "tar", "level", "bytes",
    ];
    let mut text = String::new();
    let mut seed: usize = 7;
    for _ in 0..WORDS {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        text.push_str(words[(seed >> 16) % words.len()]);
        text.push(' ');
    }
    let path = folder.join("wordy.txt");
    fs::write(&path, text).unwrap();
    path
}

const WORDS: usize = 100_000;

#[test]
fn a_higher_level_makes_a_smaller_archive() {
    let folder = sources("level-size");
    let wordy = [wordy(&folder)];
    for (packing, name) in [(Packing::TarZstd, "tar.zst"), (Packing::ZipZstd, "zip")] {
        let size = |level| {
            let archive = folder.join(format!("{level}.{name}"));
            let recipe = Recipe {
                level,
                ..recipe(packing)
            };
            archiver()
                .compress(&wordy, &archive, &recipe, go())
                .unwrap();
            fs::metadata(&archive).unwrap().len()
        };
        assert!(size(9) < size(1), "{name}");
    }
    fs::remove_dir_all(&folder).ok();
}

#[test]
fn each_file_is_told_with_its_bytes() {
    let folder = sources("told");
    for (packing, name) in [
        (Packing::ZipDeflate, "a.zip"),
        (Packing::TarGzip, "a.tar.gz"),
    ] {
        let mut told = Vec::new();
        archiver()
            .compress(
                &[folder.join("docs"), folder.join("notes.txt")],
                &folder.join(name),
                &recipe(packing),
                |extracting| {
                    if let Extracting::Entry(bytes) = extracting {
                        told.push(bytes);
                    }
                    Flow::Go
                },
            )
            .unwrap();
        told.sort_unstable();
        assert_eq!(told, [0, 0, 0, 4, 5, 5], "{name}");
    }
    fs::remove_dir_all(&folder).ok();
}

#[test]
fn a_zip_carries_dos_times_too() {
    let folder = sources("dos-time");
    let archive = make(&folder, "a.zip", &recipe(Packing::ZipDeflate)).unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(&archive).unwrap()).unwrap();
    let year = zip
        .by_name("notes.txt")
        .unwrap()
        .last_modified()
        .unwrap()
        .year();
    assert!(year > FIRST_DOS_YEAR);
    fs::remove_dir_all(&folder).ok();
}

const FIRST_DOS_YEAR: u16 = 1980;

#[test]
fn a_file_tar_cant_read_is_denied() {
    let folder = sources("tar-denied");
    let secret = folder.join("secret.txt");
    fs::write(&secret, b"s").unwrap();
    fs::set_permissions(&secret, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::File::open(&secret).is_err() {
        assert_eq!(
            archiver().compress(
                std::slice::from_ref(&secret),
                &folder.join("a.tar.gz"),
                &recipe(Packing::TarGzip),
                go()
            ),
            Err(ArchiveFailure::Denied)
        );
    }
    fs::remove_dir_all(&folder).ok();
}

#[test]
fn each_zip_packing_uses_its_own_method() {
    let folder = sources("methods");
    for (packing, name, method) in [
        (
            Packing::ZipDeflate,
            "a.zip",
            zip::CompressionMethod::Deflated,
        ),
        (Packing::ZipZstd, "z.zip", zip::CompressionMethod::Zstd),
    ] {
        let archive = make(&folder, name, &recipe(packing)).unwrap();
        let mut zip = zip::ZipArchive::new(fs::File::open(&archive).unwrap()).unwrap();
        assert_eq!(zip.by_name("notes.txt").unwrap().compression(), method);
    }
    fs::remove_dir_all(&folder).ok();
}
