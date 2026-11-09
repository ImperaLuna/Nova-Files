use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt as _;

use super::*;
use crate::ops::test_folder::TestFolder;

#[test]
fn escapes_every_byte_a_line_cant_hold() {
    assert_eq!(escape(b"/home/me/a.txt"), b"/home/me/a.txt");
    assert_eq!(escape(b"My Photos"), b"My%20Photos");
    assert_eq!(escape(b"100%"), b"100%25");
    assert_eq!(escape(b"a\nb\tc\x7f"), b"a%0Ab%09c%7F");
    assert_eq!(escape(b"caf\xe9"), b"caf%E9");
    assert_eq!(escape("é".as_bytes()), b"%C3%A9");
    assert_eq!(escape(b"!~"), b"!~");
}

#[test]
fn writes_each_record_as_a_line_and_goes_when_finished() {
    let folder = TestFolder::new("journal");
    let jobs = folder.path().join("state/jobs");
    let mut journal = Journal::start(&jobs, JobId(4)).unwrap();
    let odd = Path::new(OsStr::from_bytes(b"/a b/\xff"));

    journal.record(Record::Operation("copy")).unwrap();
    journal.record(Record::Into(Path::new("/into"))).unwrap();
    journal.record(Record::Source(odd)).unwrap();
    journal.record(Record::Part(Path::new("/into/.p"))).unwrap();
    journal.record(Record::Made(Path::new("/into/x"))).unwrap();
    journal
        .record(Record::Backup(Path::new("/into/.b"), Path::new("/into/y")))
        .unwrap();

    let file = jobs.join("4");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "nova-files-job 1\ncopy\ninto /into\nsource /a%20b/%FF\npart /into/.p\nmade /into/x\nbackup /into/.b /into/y\n"
    );
    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        FILE_MODE
    );
    assert_eq!(
        fs::metadata(&jobs).unwrap().permissions().mode() & 0o777,
        FOLDER_MODE
    );
    assert!(Journal::start(&jobs, JobId(4)).is_err());

    journal.finish();
    assert!(!file.exists());
}

#[test]
fn syncs_once_a_second_at_most() {
    let synced = Instant::now();

    assert!(!sync_due(synced, synced));
    assert!(!sync_due(synced, synced + SYNC_EVERY / 2));
    assert!(sync_due(synced, synced + SYNC_EVERY));
}

#[test]
fn a_backup_is_synced_at_once() {
    let folder = TestFolder::new("journal-backup-sync");
    let mut journal = Journal::start(folder.path(), JobId(1)).unwrap();
    let started = journal.synced;

    journal.record(Record::Made(Path::new("/a"))).unwrap();
    assert_eq!(journal.synced, started, "not yet due");
    journal
        .record(Record::Backup(Path::new("/.b"), Path::new("/b")))
        .unwrap();
    assert!(journal.synced > started);
}
