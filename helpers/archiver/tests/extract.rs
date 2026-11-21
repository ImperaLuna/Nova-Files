#![expect(clippy::unwrap_used, reason = "test setup should fail loudly")]

use std::fs::{self, File};
use std::io::Write as _;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use nova_files::{ArchiveFailure, Archiver, Extracting, Flow};
use nova_files_test_folder::TestFolder;
use zip::write::{FullFileOptions, SimpleFileOptions};

const DAY: i64 = 86_400;
const QUICK: Duration = Duration::from_secs(5);

fn archiver() -> Archiver {
    Archiver::at(PathBuf::from(env!("CARGO_BIN_EXE_nova-files-archiver")))
}

fn folder(name: &str) -> (TestFolder, PathBuf) {
    let held = TestFolder::new(&format!("archiver-extract-{name}"));
    let folder = held.path().to_path_buf();
    fs::create_dir_all(folder.join("into")).unwrap();
    (held, folder)
}

fn writable(path: &Path) {
    if path.is_dir() && !path.is_symlink() {
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        for entry in fs::read_dir(path).unwrap() {
            writable(&entry.unwrap().path());
        }
    }
}

fn header(kind: tar::EntryType, mode: u32, mtime: u64, size: u64) -> tar::Header {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(kind);
    header.set_mode(mode);
    header.set_mtime(mtime);
    header.set_size(size);
    header
}

const MOST_WAITS: usize = 100;

fn extract(archive: &Path, into: &Path, password: Option<&str>) -> Result<u64, ArchiveFailure> {
    let mut entries = 0;
    let mut waits = 0;
    archiver()
        .extract(archive, into, password, |extracting| {
            match extracting {
                Extracting::Entry(_) => entries += 1,
                Extracting::Waiting => waits += 1,
                Extracting::Held => {}
            }
            if waits < MOST_WAITS {
                Flow::Go
            } else {
                Flow::Stop
            }
        })
        .map(|()| entries)
}

#[test]
fn unpacks_a_tar_with_its_permissions_times_and_links() {
    let (_held, folder) = folder("tar");
    let mut builder = tar::Builder::new(Vec::new());
    let mut docs = header(tar::EntryType::Directory, 0o750, 1_700_000_000, 0);
    builder.append_data(&mut docs, "docs/", &b""[..]).unwrap();
    let mut file = header(tar::EntryType::Regular, 0o640, 1_700_000_100, 5);
    builder
        .append_data(&mut file, "docs/a.txt", &b"hello"[..])
        .unwrap();
    let mut link = header(tar::EntryType::Symlink, 0o777, 0, 0);
    builder
        .append_link(&mut link, "docs/to-a", "a.txt")
        .unwrap();
    let mut hard = header(tar::EntryType::Link, 0o640, 0, 0);
    builder
        .append_link(&mut hard, "docs/same", "docs/a.txt")
        .unwrap();
    let mut deep = header(tar::EntryType::Regular, 0o600, 1_700_000_200, 2);
    builder
        .append_data(&mut deep, "made/on/way.txt", &b"hi"[..])
        .unwrap();
    let archive = folder.join("a.tar.zst");
    let tar = builder.into_inner().unwrap();
    fs::write(&archive, zstd::encode_all(&tar[..], 0).unwrap()).unwrap();
    let into = folder.join("into");

    assert_eq!(extract(&archive, &into, None), Ok(5));

    let docs = into.join("docs");
    assert_eq!(fs::read(docs.join("a.txt")).unwrap(), b"hello");
    let file = fs::metadata(docs.join("a.txt")).unwrap();
    assert_eq!(file.mode() & 0o777, 0o640);
    assert_eq!(file.mtime(), 1_700_000_100);
    let docs_meta = fs::metadata(&docs).unwrap();
    assert_eq!(docs_meta.mode() & 0o777, 0o750);
    assert_eq!(docs_meta.mtime(), 1_700_000_000);
    assert_eq!(
        fs::read_link(docs.join("to-a")).unwrap(),
        Path::new("a.txt")
    );
    assert_eq!(fs::metadata(docs.join("same")).unwrap().ino(), file.ino());
    assert_eq!(fs::read(into.join("made/on/way.txt")).unwrap(), b"hi");
}

#[test]
fn unpacks_a_zip_and_its_locked_entries_with_the_password() {
    let (_held, folder) = folder("zip");
    let archive = folder.join("a.zip");
    let mut zip = zip::ZipWriter::new(File::create(&archive).unwrap());
    let options = SimpleFileOptions::default();
    zip.add_directory("docs/", options.unix_permissions(0o755))
        .unwrap();
    zip.start_file("docs/a.txt", options.unix_permissions(0o644))
        .unwrap();
    zip.write_all(b"hello").unwrap();
    zip.add_symlink("docs/to-a", "a.txt", options).unwrap();
    zip.start_file(
        "secret.txt",
        options.with_aes_encryption(zip::AesMode::Aes256, "pw"),
    )
    .unwrap();
    zip.write_all(b"hidden").unwrap();
    zip.finish().unwrap();
    let into = folder.join("into");

    assert_eq!(
        extract(&archive, &into, None),
        Err(ArchiveFailure::NeedsPassword)
    );
    assert_eq!(
        extract(&archive, &into, Some("wrong")),
        Err(ArchiveFailure::WrongPassword)
    );
    assert_eq!(extract(&archive, &into, Some("pw")), Ok(4));
    assert_eq!(fs::read(into.join("docs/a.txt")).unwrap(), b"hello");
    assert_eq!(
        fs::read_link(into.join("docs/to-a")).unwrap(),
        Path::new("a.txt")
    );
    assert_eq!(fs::read(into.join("secret.txt")).unwrap(), b"hidden");
}

#[test]
fn a_hard_link_to_its_own_name_keeps_the_file() {
    let (_held, folder) = folder("self-link");
    let mut builder = tar::Builder::new(Vec::new());
    let mut file = header(tar::EntryType::Regular, 0o644, 0, 1);
    builder.append_data(&mut file, "a", &b"x"[..]).unwrap();
    let mut hard = header(tar::EntryType::Link, 0o644, 0, 0);
    builder.append_link(&mut hard, "./a", "a").unwrap();
    let archive = folder.join("self.tar");
    fs::write(&archive, builder.into_inner().unwrap()).unwrap();
    let into = folder.join("into");

    assert_eq!(extract(&archive, &into, None), Ok(2));
    assert_eq!(fs::read(into.join("a")).unwrap(), b"x");
}

#[test]
fn unpacks_a_zip_with_no_password_and_its_times() {
    let (_held, folder) = folder("zip-times");
    let archive = folder.join("a.zip");
    let mut zip = zip::ZipWriter::new(File::create(&archive).unwrap());
    let wall = zip::DateTime::from_date_and_time(2023, 11, 14, 22, 13, 20).unwrap();
    let options = FullFileOptions::default().last_modified_time(wall);
    zip.start_file("dos.txt", options.clone()).unwrap();
    zip.write_all(b"dos").unwrap();
    let stamped = options
        .with_extended_timestamp(Some(1_600_000_000), None, None)
        .unwrap();
    zip.start_file("unix.txt", stamped).unwrap();
    zip.write_all(b"unix").unwrap();
    zip.finish().unwrap();
    let into = folder.join("into");

    assert_eq!(extract(&archive, &into, None), Ok(2));
    assert_eq!(fs::read(into.join("dos.txt")).unwrap(), b"dos");
    let unix = fs::metadata(into.join("unix.txt")).unwrap();
    assert_eq!(unix.mtime(), 1_600_000_000);
    let dos = fs::metadata(into.join("dos.txt")).unwrap().mtime();
    assert!((1_700_000_000 - DAY..=1_700_000_000 + DAY).contains(&dos));
}

#[test]
fn nothing_lands_outside_the_folder() {
    let (_held, folder) = folder("hostile");
    let outside = folder.join("outside");
    fs::create_dir(&outside).unwrap();
    let mut builder = tar::Builder::new(Vec::new());
    let mut up = header(tar::EntryType::Regular, 0o644, 0, 1);
    up.as_mut_bytes()[..12].copy_from_slice(b"../up.txt\0\0\0");
    up.set_cksum();
    builder.append(&up, &b"x"[..]).unwrap();
    let mut link = header(tar::EntryType::Symlink, 0o777, 0, 0);
    builder
        .append_link(&mut link, "out", outside.to_str().unwrap())
        .unwrap();
    let mut through = header(tar::EntryType::Regular, 0o644, 0, 1);
    builder
        .append_data(&mut through, "out/x.txt", &b"x"[..])
        .unwrap();
    let mut hard = header(tar::EntryType::Link, 0o644, 0, 0);
    builder
        .append_link(&mut hard, "hard", "../outside/y.txt")
        .unwrap();
    let archive = folder.join("a.tar");
    fs::write(&archive, builder.into_inner().unwrap()).unwrap();
    let into = folder.join("into");

    extract(&archive, &into, None).unwrap();

    assert!(!folder.join("up.txt").exists());
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    assert_eq!(fs::read(into.join("out/x.txt")).unwrap(), b"x");
    assert!(!into.join("hard").exists());
}

#[test]
fn a_later_entry_takes_the_name_but_not_a_folders() {
    let (_held, folder) = folder("later");
    let mut builder = tar::Builder::new(Vec::new());
    let mut locked = header(tar::EntryType::Directory, 0o555, 0, 0);
    builder.append_data(&mut locked, "ro/", &b""[..]).unwrap();
    for contents in [&b"old"[..], &b"new"[..]] {
        let mut file = header(tar::EntryType::Regular, 0o644, 0, 3);
        builder
            .append_data(&mut file, "ro/a.txt", contents)
            .unwrap();
    }
    let mut file = header(tar::EntryType::Regular, 0o644, 0, 1);
    builder.append_data(&mut file, "ro", &b"x"[..]).unwrap();
    let mut link = header(tar::EntryType::Symlink, 0o777, 0, 0);
    builder.append_link(&mut link, "ro", "elsewhere").unwrap();
    let archive = folder.join("a.tar");
    fs::write(&archive, builder.into_inner().unwrap()).unwrap();
    let into = folder.join("into");

    extract(&archive, &into, None).unwrap();

    assert_eq!(fs::read(into.join("ro/a.txt")).unwrap(), b"new");
    assert_eq!(fs::metadata(into.join("ro")).unwrap().mode() & 0o777, 0o555);
    writable(&folder);
}

fn many(folder: &Path, count: usize) -> PathBuf {
    let mut builder = tar::Builder::new(Vec::new());
    for index in 0..count {
        let mut file = header(tar::EntryType::Regular, 0o644, 0, 1);
        builder
            .append_data(&mut file, format!("{index}.txt"), &b"x"[..])
            .unwrap();
    }
    let archive = folder.join("many.tar");
    fs::write(&archive, builder.into_inner().unwrap()).unwrap();
    archive
}

#[test]
fn the_caller_stops_it() {
    let (_held, folder) = folder("stop");
    let archive = many(&folder, 3);
    let result = archiver().extract(&archive, &folder.join("into"), None, |_| Flow::Stop);
    assert_eq!(result, Err(ArchiveFailure::Cancelled));
}

#[test]
fn stopping_a_held_run_ends_it_at_once() {
    let (_held, folder) = folder("stop-held");
    let archive = many(&folder, 3);
    let started = Instant::now();
    let result = archiver().extract(&archive, &folder.join("into"), None, |extracting| {
        if extracting == Extracting::Held {
            Flow::Stop
        } else {
            Flow::Hold
        }
    });
    assert_eq!(result, Err(ArchiveFailure::Cancelled));
    assert!(started.elapsed() < QUICK);
}

#[test]
fn the_caller_holds_it_then_lets_it_go_on() {
    let (_held, folder) = folder("hold");
    let archive = many(&folder, 3);
    let mut told = Vec::new();
    let result = archiver().extract(&archive, &folder.join("into"), None, |extracting| {
        told.push(extracting);
        let waits = told.iter().filter(|told| **told == Extracting::Waiting);
        if waits.count() >= MOST_WAITS {
            return Flow::Stop;
        }
        match (extracting, told.len()) {
            (_, 1) | (Extracting::Held, 2) => Flow::Hold,
            _ => Flow::Go,
        }
    });
    assert_eq!(result, Ok(()));
    assert_eq!(told[1..3], [Extracting::Held, Extracting::Held]);
    assert_eq!(
        told.iter()
            .filter(|told| matches!(told, Extracting::Entry(1)))
            .count(),
        3
    );
}

#[test]
fn damage_and_missing_folders_are_told_apart() {
    let (_held, folder) = folder("damage");
    let archive = folder.join("a.tar.gz");
    fs::write(&archive, [0x1f, 0x8b, 8, 0, 0, 0]).unwrap();
    assert_eq!(
        extract(&archive, &folder.join("into"), None),
        Err(ArchiveFailure::Broken)
    );
    let text = folder.join("a.txt");
    fs::write(&text, b"just text").unwrap();
    assert_eq!(
        extract(&text, &folder.join("into"), None),
        Err(ArchiveFailure::Unsupported)
    );
    assert_eq!(
        extract(&folder.join("none.tar"), &folder.join("into"), None),
        Err(ArchiveFailure::Gone)
    );
    let archive = many(&folder, 1);
    assert_eq!(
        extract(&archive, &folder.join("no-folder"), None),
        Err(ArchiveFailure::NotWritten)
    );
}

fn found(folder: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let mut waiting = vec![folder.to_path_buf()];
    while let Some(next) = waiting.pop() {
        for entry in fs::read_dir(next).unwrap() {
            let path = entry.unwrap().path();
            names.push(
                path.strip_prefix(folder)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            );
            if path.is_dir() {
                waiting.push(path);
            }
        }
    }
    names.sort();
    names
}

#[test]
fn picked_entries_come_out_with_all_inside_them_and_nothing_else() {
    let (_held, folder) = folder("picked");
    let mut builder = tar::Builder::new(Vec::new());
    for (path, contents) in [
        ("docs/a.txt", "a"),
        ("docs/inner/b.txt", "b"),
        ("docsx/c.txt", "c"),
        ("top.txt", "t"),
    ] {
        let mut header = header(tar::EntryType::Regular, 0o644, 0, contents.len() as u64);
        builder
            .append_data(&mut header, path, contents.as_bytes())
            .unwrap();
    }
    let tar_archive = folder.join("a.tar");
    fs::write(&tar_archive, builder.into_inner().unwrap()).unwrap();
    let zip_archive = folder.join("a.zip");
    let mut zip = zip::ZipWriter::new(File::create(&zip_archive).unwrap());
    for (path, contents) in [("docs/a.txt", "a"), ("docsx/c.txt", "c"), ("top.txt", "t")] {
        zip.start_file(path, SimpleFileOptions::default()).unwrap();
        zip.write_all(contents.as_bytes()).unwrap();
    }
    zip.finish().unwrap();

    let picked = [PathBuf::from("docs"), PathBuf::from("top.txt")];
    for (archive, expected) in [
        (
            &tar_archive,
            vec![
                "docs",
                "docs/a.txt",
                "docs/inner",
                "docs/inner/b.txt",
                "top.txt",
            ],
        ),
        (&zip_archive, vec!["docs", "docs/a.txt", "top.txt"]),
    ] {
        let into = folder.join("into");
        fs::remove_dir_all(&into).unwrap();
        fs::create_dir(&into).unwrap();
        archiver()
            .extract_picked((archive, &picked), &into, None, |_| Flow::Go)
            .unwrap();
        assert_eq!(found(&into), expected, "{}", archive.display());
    }
}
