//! The change log: what changed on disk since the index was saved, in the
//! order it happened, for any number of readers.
//!
//! The watcher appends [`Change`]s with [`ChangeLog::append`]; readers
//! follow it with a [`LogReader`], each with its own place in the file, so
//! search, and later content search, read the same log at their own pace.
//! Nobody rewrites it: when the index is saved again, the log starts over
//! in a new file, renamed over the old one.
//!
//! ```text
//! header  LOG_HEADER_LEN bytes  magic, version, the index's crawl start
//! record  length (u32, the whole record), what happened, the entry's
//!         kind and flags, device, inode, size, modified, created, then
//!         the path; a move holds both paths with a NUL between them
//! ```
//!
//! The header's crawl start ties the log to one index: a log for another
//! index is started over, not read. A record is written with one `write`
//! on a file opened for appending, and a reader stops at a record that
//! isn't whole yet, so it never reads half a change.

use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{FileExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};

use crate::format::{self, NO_STAMP, i64_at, kind_byte, kind_from, put, u32_at, u64_at};
use crate::{Details, FileId, Flags, Stamp};

const LOG_MAGIC: [u8; LOG_MAGIC_LEN] = *b"NOVALOG\0";
const LOG_MAGIC_LEN: usize = 8;
const LOG_VERSION: u32 = 1;
pub const LOG_HEADER_LEN: usize = 24;
const H_VERSION: usize = 8;
const H_CRAWLED: usize = 16;

/// The fixed part of a record, before its path.
const RECORD_FIXED: usize = 48;
const R_OP: usize = 4;
const R_KIND: usize = 5;
const R_FLAGS: usize = 6;
const R_DEVICE: usize = 8;
const R_INODE: usize = 16;
const R_SIZE: usize = 24;
const R_MODIFIED: usize = 32;
const R_CREATED: usize = 40;

const OP_ADDED: u8 = 1;
const OP_CHANGED: u8 = 2;
const OP_REMOVED: u8 = 3;
const OP_MOVED: u8 = 4;
const OP_RESCAN: u8 = 5;

/// Between the two paths of a move; no path holds one.
const PATH_GAP: u8 = 0;
const FILE_MODE: u32 = 0o600;
/// How much a reader reads at a time.
const READ_CHUNK: usize = 64 * 1024;

/// One change on disk. Paths are whole paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// The entry exists, with these details. It may have existed already:
    /// treat it as "now looks like this".
    Added { path: PathBuf, details: Details },
    /// An indexed entry's contents or details changed.
    Changed { path: PathBuf, details: Details },
    /// The entry, and anything inside it, is gone from the index.
    Removed { path: PathBuf },
    /// The entry, and anything inside it, moved.
    Moved { from: PathBuf, to: PathBuf },
    /// Changes under `folder` were lost (the kernel's event queue
    /// overflowed); only reading it again tells what is there.
    Rescan { folder: PathBuf },
}

/// The writing end of a change log.
#[derive(Debug)]
pub struct ChangeLog {
    file: File,
}

impl ChangeLog {
    /// Starts a new, empty log at `path` for the index whose crawl started
    /// at `crawled`, replacing any log there in one step.
    ///
    /// # Errors
    /// When the file can't be written.
    pub fn create(path: &Path, crawled: Option<Stamp>) -> io::Result<Self> {
        let mut temp = path.as_os_str().to_owned();
        temp.push(".new");
        let temp = PathBuf::from(temp);
        // Left by a crash; if it can't be removed, creating it says why.
        fs::remove_file(&temp).ok();
        let mut file = OpenOptions::new()
            .append(true)
            .create_new(true)
            .mode(FILE_MODE)
            .open(&temp)?;
        let written = file
            .write_all(&header(crawled))
            .and_then(|()| fs::rename(&temp, path));
        if written.is_err() {
            fs::remove_file(&temp).ok();
        }
        written.map(|()| Self { file })
    }

    /// Adds `change` to the end of the log.
    ///
    /// # Errors
    /// When the write fails, for example on a full disk.
    pub fn append(&mut self, change: &Change) -> io::Result<()> {
        self.file.write_all(&record(change)?)
    }
}

/// A reader's place in a change log.
#[derive(Debug)]
pub struct LogReader {
    file: File,
    crawled: Option<Stamp>,
    at: u64,
}

impl LogReader {
    /// Opens the log at `path` from its first change.
    ///
    /// # Errors
    /// When it can't be read, or isn't a change log of this version.
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let mut start = [0; LOG_HEADER_LEN];
        file.read_exact_at(&mut start, 0)?;
        let crawled = read_header(&start).ok_or_else(|| damaged("not a change log"))?;
        let crawled = format::stamp(crawled);
        Ok(Self {
            file,
            crawled,
            at: u64::try_from(LOG_HEADER_LEN).map_err(io::Error::other)?,
        })
    }

    /// The crawl start of the index this log belongs to.
    pub const fn crawled(&self) -> Option<Stamp> {
        self.crawled
    }

    /// Every whole change added since the last call.
    ///
    /// # Errors
    /// When the file can't be read or holds a damaged record.
    pub fn read_new(&mut self) -> io::Result<Vec<Change>> {
        let mut bytes = Vec::new();
        let mut chunk = vec![0; READ_CHUNK];
        loop {
            let at = self.at + u64::try_from(bytes.len()).map_err(io::Error::other)?;
            let read = self.file.read_at(&mut chunk, at)?;
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(chunk.get(..read).unwrap_or_default());
        }
        let mut changes = Vec::new();
        let mut rest = bytes.as_slice();
        while let Some((change, len)) = parse(rest)? {
            changes.push(change);
            rest = rest.get(len..).unwrap_or_default();
            self.at += u64::try_from(len).map_err(io::Error::other)?;
        }
        Ok(changes)
    }
}

fn header(crawled: Option<Stamp>) -> [u8; LOG_HEADER_LEN] {
    let mut out = [0; LOG_HEADER_LEN];
    put(&mut out, 0, &LOG_MAGIC);
    put(&mut out, H_VERSION, &LOG_VERSION.to_le_bytes());
    put(&mut out, H_CRAWLED, &format::stored(crawled).to_le_bytes());
    out
}

/// The crawl start a log header names, as stored, or `None` when it
/// isn't one.
fn read_header(bytes: &[u8]) -> Option<i64> {
    let magic_ok = bytes.get(..LOG_MAGIC_LEN) == Some(LOG_MAGIC.as_slice());
    let version_ok = u32_at(bytes, H_VERSION) == Some(LOG_VERSION);
    (magic_ok && version_ok).then(|| i64_at(bytes, H_CRAWLED).unwrap_or(NO_STAMP))
}

/// One change as a record.
fn record(change: &Change) -> io::Result<Vec<u8>> {
    let (op, details, path) = match change {
        Change::Added { path, details } => (OP_ADDED, Some(details), path_bytes(path)),
        Change::Changed { path, details } => (OP_CHANGED, Some(details), path_bytes(path)),
        Change::Removed { path } => (OP_REMOVED, None, path_bytes(path)),
        Change::Rescan { folder } => (OP_RESCAN, None, path_bytes(folder)),
        Change::Moved { from, to } => {
            let mut both = path_bytes(from);
            both.push(PATH_GAP);
            both.extend_from_slice(&path_bytes(to));
            (OP_MOVED, None, both)
        }
    };
    let len = u32::try_from(RECORD_FIXED + path.len()).map_err(io::Error::other)?;
    let mut out = vec![0; RECORD_FIXED];
    put(&mut out, 0, &len.to_le_bytes());
    put(&mut out, R_OP, &[op]);
    if let Some(details) = details {
        put(&mut out, R_KIND, &[kind_byte(details.kind)]);
        put(&mut out, R_FLAGS, &[details.flags.0]);
        put(&mut out, R_DEVICE, &details.id.device.to_le_bytes());
        put(&mut out, R_INODE, &details.id.inode.to_le_bytes());
        put(&mut out, R_SIZE, &details.size.to_le_bytes());
        put(
            &mut out,
            R_MODIFIED,
            &format::stored(details.modified).to_le_bytes(),
        );
        put(
            &mut out,
            R_CREATED,
            &format::stored(details.created).to_le_bytes(),
        );
    }
    out.extend_from_slice(&path);
    Ok(out)
}

/// The first record in `bytes` and its length, or `None` when `bytes`
/// doesn't hold a whole one yet.
fn parse(bytes: &[u8]) -> io::Result<Option<(Change, usize)>> {
    let Some(len) = u32_at(bytes, 0) else {
        return Ok(None);
    };
    let len = usize::try_from(len).map_err(io::Error::other)?;
    if len < RECORD_FIXED {
        return Err(damaged("a record shorter than its fixed part"));
    }
    let Some(record) = bytes.get(..len) else {
        return Ok(None);
    };
    let path = record.get(RECORD_FIXED..).unwrap_or_default();
    let whole = |bytes: &[u8]| PathBuf::from(OsStr::from_bytes(bytes));
    let change = match record.get(R_OP).copied() {
        Some(OP_ADDED) => Change::Added {
            path: whole(path),
            details: details(record)?,
        },
        Some(OP_CHANGED) => Change::Changed {
            path: whole(path),
            details: details(record)?,
        },
        Some(OP_REMOVED) => Change::Removed { path: whole(path) },
        Some(OP_RESCAN) => Change::Rescan {
            folder: whole(path),
        },
        Some(OP_MOVED) => {
            let gap = path
                .iter()
                .position(|&byte| byte == PATH_GAP)
                .ok_or_else(|| damaged("a move with one path"))?;
            Change::Moved {
                from: whole(path.get(..gap).unwrap_or_default()),
                to: whole(path.get(gap + 1..).unwrap_or_default()),
            }
        }
        _ => return Err(damaged("a record of no known change")),
    };
    Ok(Some((change, len)))
}

fn details(record: &[u8]) -> io::Result<Details> {
    let kind = record
        .get(R_KIND)
        .copied()
        .and_then(kind_from)
        .ok_or_else(|| damaged("a record of no known kind"))?;
    let number = |at| u64_at(record, at).unwrap_or_default();
    let time = |at| format::stamp(i64_at(record, at).unwrap_or(NO_STAMP));
    Ok(Details {
        kind,
        flags: Flags(record.get(R_FLAGS).copied().unwrap_or_default()),
        id: FileId {
            device: number(R_DEVICE),
            inode: number(R_INODE),
        },
        size: number(R_SIZE),
        modified: time(R_MODIFIED),
        created: time(R_CREATED),
    })
}

fn path_bytes(path: &Path) -> Vec<u8> {
    path.as_os_str().as_bytes().to_vec()
}

fn damaged(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("change log: {what}"))
}

#[cfg(test)]
mod tests;
