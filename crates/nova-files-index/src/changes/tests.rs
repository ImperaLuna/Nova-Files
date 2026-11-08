//! Records written and read back, a reader that waits for a record to be
//! whole, and damaged logs.

use super::*;
use crate::Kind;

const LONG_LOG: usize = 4_000;
const CRAWLED: Option<Stamp> = Some(Stamp(1_700_000_000_000_000_000));
/// Details with every field set to something that would show a mix-up.
const DEVICE: u64 = 3;
const INODE: u64 = 1 << 40;
const SIZE: u64 = 1 << 33;
const MODIFIED: Stamp = Stamp(-5);

fn details(kind: Kind) -> Details {
    Details {
        kind,
        flags: Flags::UNREADABLE,
        id: FileId {
            device: DEVICE,
            inode: INODE,
        },
        size: SIZE,
        modified: Some(MODIFIED),
        created: None,
    }
}

fn every_change() -> Vec<Change> {
    vec![
        Change::Added {
            path: PathBuf::from("/home/me/a.txt"),
            details: details(Kind::File),
        },
        Change::Added {
            path: PathBuf::from("/home/me/docs"),
            details: details(Kind::Folder),
        },
        Change::Changed {
            path: PathBuf::from("/home/me/link"),
            details: details(Kind::Link),
        },
        Change::Removed {
            path: PathBuf::from("/home/me/old"),
        },
        Change::Moved {
            from: PathBuf::from("/home/me/x"),
            to: PathBuf::from("/home/me/y/x"),
        },
        Change::Rescan {
            folder: PathBuf::from("/home/me"),
        },
    ]
}

/// A temp file path, removed when dropped.
struct TempLog(PathBuf);

impl TempLog {
    fn new(name: &str) -> Self {
        Self(std::env::temp_dir().join(format!("nova-files-log-{name}-{}", std::process::id())))
    }
}

impl Drop for TempLog {
    fn drop(&mut self) {
        fs::remove_file(&self.0).ok();
    }
}

#[test]
fn every_change_reads_back_as_written() {
    let path = TempLog::new("round-trip");
    let mut log = ChangeLog::create(&path.0, CRAWLED).unwrap();
    let mut reader = LogReader::open(&path.0).unwrap();
    assert_eq!(reader.crawled(), CRAWLED);
    assert_eq!(reader.read_new().unwrap(), []);
    for change in every_change() {
        log.append(&change).unwrap();
    }
    assert_eq!(reader.read_new().unwrap(), every_change());
    assert_eq!(reader.read_new().unwrap(), []);
    // A second reader reads it all from the start, on its own.
    assert_eq!(
        LogReader::open(&path.0).unwrap().read_new().unwrap(),
        every_change()
    );
}

#[test]
fn a_reader_waits_for_a_record_to_be_whole() {
    let path = TempLog::new("half");
    ChangeLog::create(&path.0, None).unwrap();
    let mut reader = LogReader::open(&path.0).unwrap();
    assert_eq!(reader.crawled(), None);
    let whole = record(&every_change()[0]).unwrap();
    let mut file = OpenOptions::new().append(true).open(&path.0).unwrap();
    let (first, second) = whole.split_at(RECORD_FIXED + 2);
    for part in [&first[..2], &first[2..]] {
        file.write_all(part).unwrap();
        assert_eq!(reader.read_new().unwrap(), []);
    }
    file.write_all(second).unwrap();
    assert_eq!(reader.read_new().unwrap(), &every_change()[..1]);
}

#[test]
fn a_long_log_is_read_in_full() {
    let path = TempLog::new("long");
    let mut log = ChangeLog::create(&path.0, CRAWLED).unwrap();
    let change = Change::Removed {
        path: PathBuf::from("/home/me/a-fairly-long-folder-name/with-a-file-inside.txt"),
    };
    // Past what one read of the file returns at once.
    let count = LONG_LOG;
    for _ in 0..count {
        log.append(&change).unwrap();
    }
    assert_eq!(
        LogReader::open(&path.0).unwrap().read_new().unwrap().len(),
        count
    );
}

#[test]
fn creating_starts_the_log_over() {
    let path = TempLog::new("over");
    let mut log = ChangeLog::create(&path.0, CRAWLED).unwrap();
    log.append(&every_change()[0]).unwrap();
    fs::write(format!("{}.new", path.0.display()), b"left by a crash").unwrap();
    ChangeLog::create(&path.0, None).unwrap();
    let mut reader = LogReader::open(&path.0).unwrap();
    assert_eq!(reader.crawled(), None);
    assert_eq!(reader.read_new().unwrap(), []);
    assert!(!Path::new(&format!("{}.new", path.0.display())).exists());
}

#[test]
fn a_file_that_isnt_a_log_is_refused() {
    let path = TempLog::new("other");
    fs::write(&path.0, [b'x'; LOG_HEADER_LEN]).unwrap();
    assert_eq!(
        LogReader::open(&path.0).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    let mut other_version = header(None);
    other_version[H_VERSION] += 1;
    fs::write(&path.0, other_version).unwrap();
    assert_eq!(
        LogReader::open(&path.0).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    fs::write(&path.0, &header(None)[..LOG_HEADER_LEN - 1]).unwrap();
    assert_eq!(
        LogReader::open(&path.0).unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );
}

#[test]
fn damaged_records_are_errors() {
    let damaged = |bytes: &[u8]| parse(bytes).unwrap_err().kind();
    let mut short = record(&every_change()[3]).unwrap();
    short[..4].copy_from_slice(&u32::try_from(RECORD_FIXED - 1).unwrap().to_le_bytes());
    assert_eq!(damaged(&short), io::ErrorKind::InvalidData);
    let mut unknown = record(&every_change()[3]).unwrap();
    unknown[R_OP] = u8::MAX;
    assert_eq!(damaged(&unknown), io::ErrorKind::InvalidData);
    let mut no_kind = record(&every_change()[0]).unwrap();
    no_kind[R_KIND] = u8::MAX;
    assert_eq!(damaged(&no_kind), io::ErrorKind::InvalidData);
    let mut one_path = record(&every_change()[3]).unwrap();
    one_path[R_OP] = OP_MOVED;
    assert_eq!(damaged(&one_path), io::ErrorKind::InvalidData);
    // Exactly the fixed part, with an empty path, is a whole record.
    let empty = record(&Change::Removed {
        path: PathBuf::new(),
    })
    .unwrap();
    assert_eq!(empty.len(), RECORD_FIXED);
    assert_eq!(parse(&empty).unwrap().unwrap().1, RECORD_FIXED);
}
