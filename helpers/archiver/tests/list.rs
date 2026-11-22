#![expect(clippy::unwrap_used, reason = "test setup should fail loudly")]

use std::fs::{self, File};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use nova_files::{ArchiveEntry, ArchiveFailure, ArchiveKind, Archiver};
use nova_files_test_folder::TestFolder;
use zip::write::SimpleFileOptions;

fn archiver() -> Archiver {
    Archiver::at(PathBuf::from(env!("CARGO_BIN_EXE_nova-files-archiver")))
}

fn folder(name: &str) -> (TestFolder, PathBuf) {
    let held = TestFolder::new(&format!("archiver-{name}"));
    let folder = held.path().to_path_buf();
    (held, folder)
}

fn local_seconds(time: &str) -> i64 {
    let out = std::process::Command::new("date")
        .args(["-d", time, "+%s"])
        .output()
        .unwrap();
    String::from_utf8(out.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

fn paths(entries: &[ArchiveEntry]) -> Vec<&Path> {
    entries.iter().map(|entry| entry.path.as_path()).collect()
}

fn tar_bytes() -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    let mut folder = tar::Header::new_gnu();
    folder.set_entry_type(tar::EntryType::Directory);
    folder.set_mode(0o750);
    folder.set_mtime(1_700_000_000);
    folder.set_size(0);
    builder.append_data(&mut folder, "docs/", &b""[..]).unwrap();
    let mut file = tar::Header::new_gnu();
    file.set_mode(0o640);
    file.set_mtime(1_700_000_100);
    file.set_size(5);
    builder
        .append_data(&mut file, "docs/a.txt", &b"hello"[..])
        .unwrap();
    let mut link = tar::Header::new_gnu();
    link.set_entry_type(tar::EntryType::Symlink);
    link.set_mode(0o777);
    link.set_size(0);
    builder
        .append_link(&mut link, "docs/to-a", "a.txt")
        .unwrap();
    builder.into_inner().unwrap()
}

fn assert_tar_listing(entries: &[ArchiveEntry]) {
    assert_eq!(
        paths(entries),
        [
            Path::new("docs"),
            Path::new("docs/a.txt"),
            Path::new("docs/to-a")
        ]
    );
    assert_eq!(entries[0].kind, ArchiveKind::Folder);
    assert_eq!(entries[0].mode, 0o750);
    assert_eq!(entries[1].kind, ArchiveKind::File);
    assert_eq!(entries[1].size, 5);
    assert_eq!(entries[1].modified, 1_700_000_100);
    assert_eq!(entries[1].mode, 0o640);
    assert_eq!(entries[2].kind, ArchiveKind::Link);
    assert_eq!(entries[2].link, Path::new("a.txt"));
    assert!(entries.iter().all(|entry| !entry.locked));
}

#[test]
fn lists_tar_in_every_wrapping() {
    let (_held, folder) = folder("tar");
    let tar = tar_bytes();
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(&tar).unwrap();
    let mut xz = lzma_rust2::XzWriter::new(Vec::new(), lzma_rust2::XzOptions::default()).unwrap();
    xz.write_all(&tar).unwrap();
    let mut bzip2 = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
    bzip2.write_all(&tar).unwrap();
    for (name, bytes) in [
        ("plain.zip", tar.clone()),
        ("a.tar.gz", gzip.finish().unwrap()),
        ("a.tar.xz", xz.finish().unwrap()),
        ("a.tar.zst", zstd::encode_all(&tar[..], 0).unwrap()),
        ("a.tar.bz2", bzip2.finish().unwrap()),
    ] {
        let path = folder.join(name);
        fs::write(&path, bytes).unwrap();
        assert_tar_listing(&archiver().list(&path).unwrap());
    }
}

#[test]
fn lists_zip_with_its_unix_details_and_locked_entries() {
    let (_held, folder) = folder("zip");
    let path = folder.join("a.zip");
    let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
    let options = SimpleFileOptions::default()
        .last_modified_time(zip::DateTime::from_date_and_time(2024, 5, 6, 7, 8, 10).unwrap());
    zip.add_directory("docs/", options.unix_permissions(0o750))
        .unwrap();
    zip.start_file("docs/a.txt", options.unix_permissions(0o640))
        .unwrap();
    zip.write_all(b"hello").unwrap();
    zip.add_symlink("docs/to-a", "a.txt", options).unwrap();
    zip.start_file(
        "secret.txt",
        options.with_aes_encryption(zip::AesMode::Aes256, "pw"),
    )
    .unwrap();
    zip.write_all(b"hidden").unwrap();
    zip.add_symlink(
        "locked-link",
        "secret.txt",
        options.with_aes_encryption(zip::AesMode::Aes256, "pw"),
    )
    .unwrap();
    zip.finish().unwrap();

    let entries = archiver().list(&path).unwrap();
    assert_eq!(
        paths(&entries),
        [
            Path::new("docs"),
            Path::new("docs/a.txt"),
            Path::new("docs/to-a"),
            Path::new("secret.txt"),
            Path::new("locked-link")
        ]
    );
    assert_eq!(entries[0].kind, ArchiveKind::Folder);
    assert_eq!(entries[0].mode, 0o750);
    assert_eq!(entries[1].size, 5);
    assert_eq!(entries[1].mode, 0o640);
    assert_eq!(entries[1].modified, local_seconds("2024-05-06T07:08:10"));
    assert_eq!(entries[2].kind, ArchiveKind::Link);
    assert_eq!(entries[2].link, Path::new("a.txt"));
    assert!(!entries[1].locked);
    assert!(entries[3].locked);
    assert_eq!(entries[4].kind, ArchiveKind::Link);
    assert!(entries[4].locked);
    assert_eq!(entries[4].link, Path::new(""));
}

#[test]
fn zip_with_no_permissions_gets_plain_ones() {
    let (_held, folder) = folder("zip-bare");
    let path = folder.join("a.zip");
    let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
    zip.add_directory("d/", SimpleFileOptions::default())
        .unwrap();
    zip.start_file("d/f", SimpleFileOptions::default()).unwrap();
    zip.finish().unwrap();
    let mut bytes = fs::read(&path).unwrap();
    let headers: Vec<usize> = bytes
        .windows(4)
        .enumerate()
        .filter(|(_, window)| *window == b"PK\x01\x02")
        .map(|(at, _)| at)
        .collect();
    for at in headers {
        bytes[at + 38..at + 42].fill(0);
    }
    fs::write(&path, bytes).unwrap();
    let entries = archiver().list(&path).unwrap();
    assert_eq!(entries[0].kind, ArchiveKind::Folder);
    assert_eq!(entries[0].mode, 0o755);
    assert_eq!(entries[1].mode, 0o644);
}

#[test]
fn zip_from_dos_gets_permissions_from_its_attributes() {
    let (_held, folder) = folder("zip-dos");
    let path = folder.join("a.zip");
    let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
    let options = SimpleFileOptions::default()
        .system(zip::System::Dos)
        .last_modified_time(zip::DateTime::default());
    zip.add_directory("d/", options).unwrap();
    zip.start_file("d/f", options).unwrap();
    zip.finish().unwrap();
    let entries = archiver().list(&path).unwrap();
    assert_eq!(entries[0].mode, 0o775);
    assert_eq!(entries[1].mode, 0o664);
    let first_dos_day = chrono::NaiveDate::from_ymd_opt(1980, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let local = chrono::TimeZone::from_local_datetime(&chrono::Local, &first_dos_day)
        .earliest()
        .unwrap();
    assert_eq!(entries[1].modified, local.timestamp());
}

#[test]
fn hostile_paths_are_left_out() {
    let (_held, folder) = folder("hostile");
    let path = folder.join("evil.tar");
    let mut builder = tar::Builder::new(Vec::new());
    for name in [&b"../escape"[..], b"/etc/evil", b"ok.txt"] {
        let mut header = tar::Header::new_old();
        header.as_old_mut().name[..name.len()].copy_from_slice(name);
        header.set_size(1);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, &b"x"[..]).unwrap();
    }
    fs::write(&path, builder.into_inner().unwrap()).unwrap();
    let entries = archiver().list(&path).unwrap();
    assert_eq!(
        paths(&entries),
        [Path::new("etc/evil"), Path::new("ok.txt")]
    );
}

#[test]
fn other_files_and_damage_are_told_apart() {
    let (_held, folder) = folder("bad");
    let text = folder.join("notes.zip");
    fs::write(&text, b"just some text, not an archive at all").unwrap();
    assert_eq!(archiver().list(&text), Err(ArchiveFailure::Unsupported));
    let cut = folder.join("cut.tar.gz");
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(&tar_bytes()).unwrap();
    let whole = gzip.finish().unwrap();
    fs::write(&cut, &whole[..whole.len() / 2]).unwrap();
    assert_eq!(archiver().list(&cut), Err(ArchiveFailure::Broken));
    assert_eq!(
        archiver().list(&folder.join("missing.zip")),
        Err(ArchiveFailure::Gone)
    );
    let nowhere = Archiver::at(folder.join("no-helper"));
    assert_eq!(nowhere.list(&text), Err(ArchiveFailure::NotRead));
}

#[test]
fn an_archive_the_user_may_not_read_is_denied() {
    use std::os::unix::fs::PermissionsExt as _;
    let (_held, folder) = folder("denied");
    let path = folder.join("secret.tar");
    fs::write(&path, tar_bytes()).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    if File::open(&path).is_err() {
        assert_eq!(archiver().list(&path), Err(ArchiveFailure::Denied));
    }
}

#[test]
fn tars_joined_end_to_end_list_whole() {
    let (_held, folder) = folder("joined");
    let path = folder.join("both.tar");
    let mut second = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(1);
    header.set_mode(0o644);
    second
        .append_data(&mut header, "second.txt", &b"x"[..])
        .unwrap();
    fs::write(&path, [tar_bytes(), second.into_inner().unwrap()].concat()).unwrap();
    let entries = archiver().list(&path).unwrap();
    assert_eq!(entries.last().unwrap().path, Path::new("second.txt"));
    assert_eq!(entries.len(), 4);
}

#[test]
fn old_tar_folders_end_in_a_slash() {
    let (_held, folder) = folder("old-tar");
    let path = folder.join("old.tar");
    let mut builder = tar::Builder::new(Vec::new());
    for (name, size) in [(&b"docs/"[..], 0), (b"docs/a.txt", 1)] {
        let mut header = tar::Header::new_old();
        header.as_old_mut().name[..name.len()].copy_from_slice(name);
        header.set_size(size);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append(&header, &b"x"[..usize::try_from(size).unwrap()])
            .unwrap();
    }
    fs::write(&path, builder.into_inner().unwrap()).unwrap();
    let entries = archiver().list(&path).unwrap();
    assert_eq!(
        paths(&entries),
        [Path::new("docs"), Path::new("docs/a.txt")]
    );
    assert_eq!(entries[0].kind, ArchiveKind::Folder);
    assert_eq!(entries[1].kind, ArchiveKind::File);
}

#[test]
fn an_empty_zip_lists_nothing() {
    let (_held, folder) = folder("empty");
    let path = folder.join("empty.zip");
    zip::ZipWriter::new(File::create(&path).unwrap())
        .finish()
        .unwrap();
    assert_eq!(archiver().list(&path), Ok(Vec::new()));
}

#[test]
fn an_empty_tar_lists_nothing() {
    let (_held, folder) = folder("empty-tar");
    let path = folder.join("empty.tar");
    fs::write(&path, tar::Builder::new(Vec::new()).into_inner().unwrap()).unwrap();
    assert_eq!(archiver().list(&path), Ok(Vec::new()));
}

#[test]
fn an_unknown_ask_is_a_usage_error() {
    let (_held, folder) = folder("usage");
    let archive = folder.join("a.tar");
    fs::write(&archive, tar_bytes()).unwrap();
    for extra in [None, Some(&folder)] {
        let status = std::process::Command::new(env!("CARGO_BIN_EXE_nova-files-archiver"))
            .arg("show")
            .arg(&archive)
            .args(extra)
            .output()
            .unwrap();
        assert_eq!(status.status.code(), Some(1));
        assert_eq!(status.stdout, b"");
    }
}
