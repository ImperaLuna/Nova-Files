use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Write as _};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use nova_files_api::JobId;
use rustix::fs::{FlockOperation, flock};

use super::plan::Entry;
use nova_files_disk::percent;

pub const FOLDER: &str = "nova-files/jobs";
const HEADER: &str = "nova-files-job 1";
const FILE_MODE: u32 = 0o600;
const FOLDER_MODE: u32 = 0o700;
const PLAIN: std::ops::RangeInclusive<u8> = b'!'..=b'~';

pub const PART_PREFIX: &str = ".nova-part-";
pub const BACKUP_PREFIX: &str = ".nova-backup-";

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
    pub const TRASH: &str = "trash";
    pub const TRASH_INFO: &str = "trash-info";
    pub const TRASHED: &str = "trashed";
    pub const RESTORE: &str = "restore";
    pub const DELETE_FROM_TRASH: &str = "delete-from-trash";
    pub const EMPTY_TRASH: &str = "empty-trash";
    pub const UNDO: &str = "undo";
    pub const PUT_BACK: &str = "put-back";

    const PAIR: usize = 2;
    const TRIPLE: usize = 3;

    pub fn fields(word: &str) -> Option<usize> {
        match word {
            COPY | MOVE | DELETE | TRASH | RESTORE | DELETE_FROM_TRASH | EMPTY_TRASH | UNDO
            | PUT_BACK | PLAN_DONE | SOURCES_DONE => Some(0),
            INTO | SOURCE | PART => Some(1),
            MADE | MOVED | BACKUP | PLANNED | TRASHED => Some(PAIR),
            TRASH_INFO => Some(TRIPLE),
            _ => None,
        }
    }
}

const ENTRY_SPLIT: char = ':';

fn entry_field(entry: Entry) -> PathBuf {
    let Entry {
        device,
        inode,
        born,
    } = entry;
    PathBuf::from(born.map_or_else(
        || format!("{device}{ENTRY_SPLIT}{inode}"),
        |born| format!("{device}{ENTRY_SPLIT}{inode}{ENTRY_SPLIT}{born}"),
    ))
}

pub fn entry_from(field: &Path) -> Option<Entry> {
    let mut parts = field.to_str()?.split(ENTRY_SPLIT);
    let entry = Entry {
        device: parts.next()?.parse().ok()?,
        inode: parts.next()?.parse().ok()?,
        born: parts.next().map(str::parse).transpose().ok()?,
    };
    parts.next().is_none().then_some(entry)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Record<'a> {
    Operation(&'static str),
    Into(&'a Path),
    Source(&'a Path),
    Part(&'a Path),
    Made(&'a Path, &'a Path),
    Moved(&'a Path, &'a Path),
    Backup(&'a Path, &'a Path),
    Planned(&'a Path, Entry),
    PlanDone,
    SourcesDone,
    TrashInfo(&'a Path, Entry, Entry),
    Trashed(&'a Path, &'a Path),
}

impl Record<'_> {
    fn line(self) -> Vec<u8> {
        let entry;
        let entries;
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
            Self::TrashInfo(path, info, item) => {
                entries = [entry_field(*info), entry_field(*item)];
                (
                    word::TRASH_INFO,
                    &[*path, entries[0].as_path(), entries[1].as_path()],
                )
            }
            Self::Trashed(from, to) => (word::TRASHED, &[*from, *to]),
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

fn escape(bytes: &[u8]) -> Vec<u8> {
    percent::escape(bytes, |byte| PLAIN.contains(&byte))
}

const SYNC_EVERY: Duration = Duration::from_secs(1);

fn sync_due(synced: Instant, now: Instant) -> bool {
    now.duration_since(synced) >= SYNC_EVERY
}

pub struct Journal {
    file: File,
    path: PathBuf,
    synced: Instant,
}

impl Journal {
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
        hold(&file)?;
        file.write_all(HEADER.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_data()?;
        File::open(folder)?.sync_all()?;
        Ok(Self {
            file,
            path,
            synced: Instant::now(),
        })
    }

    pub fn reopen(file: File, path: PathBuf, whole: u64) -> io::Result<Self> {
        file.set_len(whole)?;
        Ok(Self {
            file,
            path,
            synced: Instant::now(),
        })
    }

    pub fn record(&mut self, record: Record<'_>) -> io::Result<()> {
        self.file.write_all(&record.line())?;
        let now = Instant::now();
        if matches!(record, Record::Backup(..)) || sync_due(self.synced, now) {
            self.file.sync_data()?;
            self.synced = now;
        }
        Ok(())
    }

    pub fn sync(&mut self) -> io::Result<()> {
        self.file.sync_data()?;
        self.synced = Instant::now();
        Ok(())
    }

    pub fn finish(self) {
        remove_journal(&self.path);
    }

    pub fn into_held(self) -> (PathBuf, File) {
        (self.path, self.file)
    }
}

pub fn remove_journal(path: &Path) {
    if let Err(error) = fs::remove_file(path) {
        log::warn!("can't remove the job journal {}: {error}", path.display());
    }
}

pub fn hold(journal: &File) -> io::Result<()> {
    flock(journal, FlockOperation::NonBlockingLockExclusive).map_err(io::Error::from)
}

pub mod read;
#[cfg(any(test, feature = "test-support"))]
impl Journal {
    pub fn over(file: File, path: PathBuf) -> Self {
        Self {
            file,
            path,
            synced: Instant::now(),
        }
    }

    pub const fn synced(&self) -> Instant {
        self.synced
    }
}

#[cfg(test)]
mod tests;
