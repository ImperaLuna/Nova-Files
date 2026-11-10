//! The job journal: what a job is about to do and has done, on disk, so a
//! job cut short by a crash can be cleaned up and resumed, and a finished
//! one undone.
//!
//! Each job writes one file, `<state>/nova-files/jobs/<id>`, readable by
//! the user alone. It is a list of lines, each a record word and the
//! paths it is about. Paths are written byte by byte, with `%`, spaces, control bytes
//! and bytes outside ASCII as `%` and two hex digits, so any name, UTF-8
//! or not, fits on one line. A part file is recorded before it is made
//! and a copy after it is made, so the journal may miss a copy but never
//! claims one the job didn't make: undo can't remove what isn't the job's.
//! Records reach the disk at most a second late, so a crash may leave the
//! last part files hidden and unrecorded, but costs no data; a backup,
//! the user's own file stepping aside, is on the disk before it happens. The file is
//! removed when the job ends.
//!
//! ```text
//! nova-files-job 1
//! copy
//! into /home/me/Backup
//! source /home/me/My%20Photos
//! sources-done
//! part /home/me/Backup/.nova-part-4-0
//! made /home/me/Backup/My%20Photos /home/me/My%20Photos
//! ```
//!
//! `made` names what was made, then its source, so a copy made under
//! another name (the user kept both) is found again. A move records each
//! item it moved, from and to, instead of `made`; a delete records what
//! it was asked to delete, then each entry it planned to, with its
//! device and inode, before it removes any.
//!
//! `sources-done` follows the last source: a journal cut short before it
//! doesn't say all the job was asked to do, so it is cleaned up but not
//! resumed.
//!
//! A line with a word this version doesn't write, or with the wrong
//! number of fields, makes the whole journal unreadable: it is left
//! alone rather than half understood.
//!
//! A running job holds a lock on its journal, so a second core never
//! takes a live job for one a crash cut short.

use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Write as _};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use nova_files_api::JobId;
use rustix::fs::{FlockOperation, flock};

use super::plan::Entry;

/// The journals' folder, under the state folder.
pub const FOLDER: &str = "nova-files/jobs";
/// The first line, naming the format and its version.
const HEADER: &str = "nova-files-job 1";
const FILE_MODE: u32 = 0o600;
const FOLDER_MODE: u32 = 0o700;
/// Bytes written as themselves: printable ASCII but `%` and the space.
const PLAIN: std::ops::RangeInclusive<u8> = b'!'..=b'~';
const ESCAPE: u8 = b'%';
const HEX: &[u8; 16] = b"0123456789ABCDEF";
/// How far a byte's high hex digit is shifted.
const HIGH: u8 = 4;
const LOW_MASK: u8 = 0x0F;

/// The start of the hidden name of a part file a job writes, followed by
/// the job's id for names and the item's index.
pub const PART_PREFIX: &str = ".nova-part-";
/// The start of the hidden name a user's file steps aside to.
pub const BACKUP_PREFIX: &str = ".nova-backup-";

/// The record words, as written and read back.
pub mod word {
    pub const COPY: &str = "copy";
    pub const MOVE: &str = "move";
    pub const DELETE: &str = "delete";
    pub const INTO: &str = "into";
    pub const SOURCE: &str = "source";
    pub const PART: &str = "part";
    pub const MADE: &str = "made";
    pub const MOVED: &str = "moved";
    pub const BACKUP: &str = "backup";
    pub const PLANNED: &str = "planned";
    pub const PLAN_DONE: &str = "plan-done";
    pub const SOURCES_DONE: &str = "sources-done";

    /// The fields of a record about two paths, or a path and its entry.
    const PAIR: usize = 2;

    /// How many fields follow `word`, or `None` for a word this version
    /// doesn't write.
    pub fn fields(word: &str) -> Option<usize> {
        match word {
            COPY | MOVE | DELETE | PLAN_DONE | SOURCES_DONE => Some(0),
            INTO | SOURCE | PART => Some(1),
            MADE | MOVED | BACKUP | PLANNED => Some(PAIR),
            _ => None,
        }
    }
}

/// Splits an entry's device and inode in its field.
const ENTRY_SPLIT: char = ':';

/// `entry` as a journal field: its device, then its inode.
fn entry_field(entry: Entry) -> PathBuf {
    PathBuf::from(format!("{}{ENTRY_SPLIT}{}", entry.device, entry.inode))
}

/// The entry `field` names, or `None` when it names none.
pub fn entry_from(field: &Path) -> Option<Entry> {
    let (device, inode) = field.to_str()?.split_once(ENTRY_SPLIT)?;
    Some(Entry {
        device: device.parse().ok()?,
        inode: inode.parse().ok()?,
    })
}

/// One line of a journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Record<'a> {
    /// The operation's name, the first record after the header.
    Operation(&'static str),
    /// The folder a copy or move goes into.
    Into(&'a Path),
    /// One of the operation's sources.
    Source(&'a Path),
    /// A part file about to be written. Left behind by a crash, it goes.
    Part(&'a Path),
    /// Something the job made at the first path, from the source at the
    /// second, which undo would remove.
    Made(&'a Path, &'a Path),
    /// Something the job moved from the first path to the second, which
    /// undo would move back.
    Moved(&'a Path, &'a Path),
    /// The user's file at the second path is about to step aside to the
    /// first while its replacement takes its name. Left behind by a
    /// crash, it goes back if its name is free.
    Backup(&'a Path, &'a Path),
    /// An entry a delete planned to remove, and which entry it was. A
    /// resumed delete removes only these, never what came after, even
    /// under the same name.
    Planned(&'a Path, Entry),
    /// The delete's whole plan is recorded: a resume may keep to it.
    PlanDone,
    /// Every source is recorded: the job says all it was asked to do,
    /// so it may resume.
    SourcesDone,
}

impl Record<'_> {
    /// The record as a line, newline included.
    fn line(self) -> Vec<u8> {
        let entry;
        let (word, paths): (&str, &[&Path]) = match &self {
            Self::Operation(name) => (name, &[]),
            Self::Into(path) => (word::INTO, std::slice::from_ref(path)),
            Self::Source(path) => (word::SOURCE, std::slice::from_ref(path)),
            Self::Part(path) => (word::PART, std::slice::from_ref(path)),
            Self::Made(made, from) => (word::MADE, &[*made, *from]),
            Self::Moved(from, to) => (word::MOVED, &[*from, *to]),
            Self::Backup(backup, of) => (word::BACKUP, &[*backup, *of]),
            Self::Planned(path, planned) => {
                entry = entry_field(*planned);
                (word::PLANNED, &[*path, entry.as_path()])
            }
            Self::PlanDone => (word::PLAN_DONE, &[]),
            Self::SourcesDone => (word::SOURCES_DONE, &[]),
        };
        let mut line = word.as_bytes().to_vec();
        for path in paths {
            line.push(b' ');
            line.extend(escape(path.as_os_str().as_bytes()));
        }
        line.push(b'\n');
        line
    }
}

/// `bytes` with every byte outside [`PLAIN`] escaped.
fn escape(bytes: &[u8]) -> Vec<u8> {
    let mut escaped = Vec::with_capacity(bytes.len());
    for &byte in bytes {
        if PLAIN.contains(&byte) && byte != ESCAPE {
            escaped.push(byte);
        } else {
            escaped.extend([
                ESCAPE,
                HEX[usize::from(byte >> HIGH)],
                HEX[usize::from(byte & LOW_MASK)],
            ]);
        }
    }
    escaped
}

/// The longest a record waits to be synced to the disk.
const SYNC_EVERY: Duration = Duration::from_secs(1);

/// Whether a journal last synced at `synced` is due a sync at `now`.
fn sync_due(synced: Instant, now: Instant) -> bool {
    now.duration_since(synced) >= SYNC_EVERY
}

/// A job's open journal.
pub struct Journal {
    file: File,
    path: PathBuf,
    synced: Instant,
}

impl Journal {
    /// Starts job `job`'s journal in `folder`.
    ///
    /// # Errors
    /// When the folder or the file can't be made, or a job with this id
    /// already has a journal.
    pub fn start(folder: &Path, job: JobId) -> io::Result<Self> {
        DirBuilder::new()
            .recursive(true)
            .mode(FOLDER_MODE)
            .create(folder)?;
        let path = folder.join(job.0.to_string());
        let mut file = OpenOptions::new()
            .append(true)
            .create_new(true)
            .mode(FILE_MODE)
            .open(&path)?;
        // Before the header, so a journal that says anything is held.
        hold(&file)?;
        file.write_all(HEADER.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_data()?;
        // The journal's own entry in its folder, so a crash can't lose it.
        File::open(folder)?.sync_all()?;
        Ok(Self {
            file,
            path,
            synced: Instant::now(),
        })
    }

    /// Adds to the journal at `path` again, left by a crash and held
    /// through `file` since, as its job resumes. What follows its first
    /// `whole` bytes, a last line the crash cut short, goes first, so
    /// the next record starts a line of its own.
    ///
    /// # Errors
    /// When it can't be cut back.
    pub fn reopen(file: File, path: PathBuf, whole: u64) -> io::Result<Self> {
        file.set_len(whole)?;
        Ok(Self {
            file,
            path,
            synced: Instant::now(),
        })
    }

    /// Adds `record`, syncing the journal when it was last synced more
    /// than [`SYNC_EVERY`] ago, or at once for a backup: the user's file
    /// is about to leave its name, and a crash must not lose where to.
    ///
    /// # Errors
    /// When it can't be written.
    pub fn record(&mut self, record: Record<'_>) -> io::Result<()> {
        self.file.write_all(&record.line())?;
        let now = Instant::now();
        if matches!(record, Record::Backup(..)) || sync_due(self.synced, now) {
            self.file.sync_data()?;
            self.synced = now;
        }
        Ok(())
    }

    /// Syncs what is written so far to the disk.
    ///
    /// # Errors
    /// When it can't be synced.
    pub fn sync(&mut self) -> io::Result<()> {
        self.file.sync_data()?;
        self.synced = Instant::now();
        Ok(())
    }

    /// The job is over: its journal goes.
    pub fn finish(self) {
        if let Err(error) = fs::remove_file(&self.path) {
            log::warn!(
                "can't remove the job journal {}: {error}",
                self.path.display()
            );
        }
    }
}

/// Holds `journal` for as long as it is open, so no other core takes its
/// job for one a crash cut short.
///
/// # Errors
/// [`io::ErrorKind::WouldBlock`] when another holds it.
pub fn hold(journal: &File) -> io::Result<()> {
    flock(journal, FlockOperation::NonBlockingLockExclusive).map_err(io::Error::from)
}

pub mod read;
#[cfg(test)]
impl Journal {
    /// A journal written through `file`, which may refuse writes.
    pub fn over(file: File, path: PathBuf) -> Self {
        Self {
            file,
            path,
            synced: Instant::now(),
        }
    }

    /// When it was last synced.
    pub const fn synced(&self) -> Instant {
        self.synced
    }
}

#[cfg(test)]
mod tests;
