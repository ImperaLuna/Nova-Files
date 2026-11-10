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
    journal
        .record(Record::Made(Path::new("/into/x"), Path::new("/x")))
        .unwrap();
    journal
        .record(Record::Backup(Path::new("/into/.b"), Path::new("/into/y")))
        .unwrap();
    journal
        .record(Record::Moved(Path::new("/z"), Path::new("/into/z")))
        .unwrap();

    let file = jobs.join("4");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "nova-files-job 1\ncopy\ninto /into\nsource /a%20b/%FF\npart /into/.p\nmade /into/x /x\nbackup /into/.b /into/y\nmoved /z /into/z\n"
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

    journal
        .record(Record::Made(Path::new("/a"), Path::new("/b")))
        .unwrap();
    assert_eq!(journal.synced, started, "not yet due");
    journal
        .record(Record::Backup(Path::new("/.b"), Path::new("/b")))
        .unwrap();
    assert!(journal.synced > started);
}

#[test]
fn records_what_resumes_need() {
    let folder = TestFolder::new("journal-resume-records");
    let jobs = folder.path().join("jobs");
    let mut journal = Journal::start(&jobs, JobId(5)).unwrap();

    journal
        .record(Record::Made(Path::new("/a (2)"), Path::new("/b/a")))
        .unwrap();
    let entry = Entry {
        device: 2049,
        inode: 77,
    };
    journal
        .record(Record::Planned(Path::new("/t"), entry))
        .unwrap();
    journal.record(Record::PlanDone).unwrap();
    journal.record(Record::SourcesDone).unwrap();

    assert_eq!(
        fs::read_to_string(jobs.join("5")).unwrap(),
        "nova-files-job 1\nmade /a%20(2) /b/a\nplanned /t 2049:77\nplan-done\nsources-done\n"
    );
    let path = jobs.join("5");
    let written = read::read(&File::open(&path).unwrap(), &path).unwrap();
    assert_eq!(entry_from(&written.lines[1].paths[1]), Some(entry));
}

#[test]
fn a_journal_is_held_while_open() {
    let folder = TestFolder::new("journal-held");
    let jobs = folder.path().join("jobs");
    let journal = Journal::start(&jobs, JobId(6)).unwrap();

    let other = File::open(jobs.join("6")).unwrap();
    let error = hold(&other).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);

    drop(journal);
    hold(&other).unwrap();
}

#[test]
fn a_reopened_journal_drops_a_line_cut_short_first() {
    let folder = TestFolder::new("journal-reopen");
    let path = folder.path().join("7");
    fs::write(&path, "nova-files-job 1\ncopy\npart /into/.nova-pa").unwrap();
    let file = OpenOptions::new()
        .read(true)
        .append(true)
        .open(&path)
        .unwrap();
    let written = read::read(&file, &path).unwrap();
    assert_eq!(written.lines.len(), 1, "the cut line is left out");

    let mut journal = Journal::reopen(file, path.clone(), written.whole).unwrap();
    journal
        .record(Record::Made(Path::new("/into/x"), Path::new("/x")))
        .unwrap();

    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "nova-files-job 1\ncopy\nmade /into/x /x\n"
    );
}

#[test]
fn reads_back_what_it_wrote() {
    let folder = TestFolder::new("journal-read");
    let jobs = folder.path().join("jobs");
    let mut journal = Journal::start(&jobs, JobId(8)).unwrap();
    let odd = Path::new(OsStr::from_bytes(b"/a b/%\xff"));
    journal
        .record(Record::Moved(odd, Path::new("/to")))
        .unwrap();
    let path = jobs.join("8");

    let written = read::read(&File::open(&path).unwrap(), &path).unwrap();

    assert_eq!(
        written.lines,
        [read::Line {
            word: word::MOVED.to_owned(),
            paths: vec![odd.to_path_buf(), PathBuf::from("/to")],
        }]
    );
    assert_eq!(written.whole, fs::metadata(&path).unwrap().len());
    let size = written.whole;
    let file = File::open(&path).unwrap();
    assert!(read::read_at_most(&file, &path, size).is_ok());
    let file = File::open(&path).unwrap();
    let refused = read::read_at_most(&file, &path, size - 1).unwrap_err();
    assert_eq!(refused.kind(), io::ErrorKind::FileTooLarge);
    drop(journal);
}

#[test]
fn a_file_that_is_not_a_journal_is_refused() {
    let folder = TestFolder::new("journal-not");
    let path = folder.path().join("9");
    for text in [
        "not a journal\n",
        "nova-files-job 1\nmade /a%zz /b\n",
        "nova-files-job 1\nundo /a\n",
        "nova-files-job 1\nplan-done /a\n",
        "nova-files-job 1\nmade /a\n",
        "nova-files-job 1\nplanned /a\n",
        "nova-files-job 1\nplanned /a 1-2\n",
        "nova-files-job 1\nplanned /a 1:x\n",
        "nova-files-job 1\nplanned /a x:2\n",
    ] {
        fs::write(&path, text).unwrap();
        let error = read::read(&File::open(&path).unwrap(), &path).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
