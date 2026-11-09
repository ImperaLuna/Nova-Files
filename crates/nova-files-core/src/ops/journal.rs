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
//! part /home/me/Backup/.nova-part-4-0
//! made /home/me/Backup/My%20Photos
//! ```
//!
//! A move records each item it moved, from and to, instead; a delete
//! records only what it was asked to delete.

use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Write as _};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use nova_files_api::JobId;

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
    /// Something the job made, which undo would remove.
    Made(&'a Path),
    /// Something the job moved from the first path to the second, which
    /// undo would move back.
    Moved(&'a Path, &'a Path),
    /// The user's file at the second path is about to step aside to the
    /// first while its replacement takes its name. Left behind by a
    /// crash, it goes back if its name is free.
    Backup(&'a Path, &'a Path),
}

impl Record<'_> {
    /// The record as a line, newline included.
    fn line(self) -> Vec<u8> {
        let (word, paths): (&str, &[&Path]) = match &self {
            Self::Operation(name) => (name, &[]),
            Self::Into(path) => ("into", std::slice::from_ref(path)),
            Self::Source(path) => ("source", std::slice::from_ref(path)),
            Self::Part(path) => ("part", std::slice::from_ref(path)),
            Self::Made(path) => ("made", std::slice::from_ref(path)),
            Self::Moved(from, to) => ("moved", &[*from, *to]),
            Self::Backup(backup, of) => ("backup", &[*backup, *of]),
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

#[cfg(test)]
mod tests;
