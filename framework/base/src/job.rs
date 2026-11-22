//! File operations as the core runs them: jobs a client starts, and what
//! the core reports about them.
//!
//! A client starts a job with [`Command::StartJob`], tagging it with a
//! [`JobTag`] of its own choosing. The core answers that client with
//! [`Event::JobStarted`], naming the [`JobId`] it gave the job. Everything
//! after that is about the job, not the client, so every client that
//! watches jobs ([`Command::WatchJobs`]) hears it: a job keeps running when
//! the window that started it closes, and another window can follow it.
//!
//! [`Command::StartJob`]: crate::Command::StartJob
//! [`Command::WatchJobs`]: crate::Command::WatchJobs
//! [`Event::JobStarted`]: crate::Event::JobStarted

use std::any::Any;
use std::ffi::OsString;
use std::path::PathBuf;

use crate::access::AccessChange;

mod carried;

pub use carried::Carried;

/// Names one job request, so a client can tell which [`JobId`] is the job
/// it asked for. The client picks it; the core only echoes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobTag(pub u64);

/// Names one job. The core picks it, unique across the core's runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(pub u64);

/// What a job does to the disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    /// Give the file or folder at `path` the name `name`, in the same
    /// folder. A name already taken fails the job; nothing is replaced.
    Rename { path: PathBuf, name: OsString },
    /// Make an empty folder named `name` in the folder `inside`. A name
    /// already taken fails the job.
    NewFolder { inside: PathBuf, name: OsString },
    /// Copy each of `sources` (files, folders with all they hold, links as
    /// links) into the folder `into`, under its own name. Each file is
    /// written under a hidden name and only takes its own once whole.
    Copy {
        sources: Vec<PathBuf>,
        into: PathBuf,
    },
    /// Move each of `sources` into the folder `into`, under its own name.
    /// On one drive each is renamed; across drives it is copied as by
    /// [`Operation::Copy`], and each source item goes only once its copy
    /// is whole, folders last.
    Move {
        sources: Vec<PathBuf>,
        into: PathBuf,
    },
    /// Delete each of `paths`, folders with all they hold, for good: not
    /// to the trash. A link goes, never what it points to.
    Delete { paths: Vec<PathBuf> },
    /// Change the owner, group or permissions of each of `paths`, and,
    /// as `change` reaches, of what the folders among them hold. A link
    /// is never changed or followed. Running it again does nothing more,
    /// so a crash leaves no job to resume: the user may simply ask again.
    /// Its progress counts what it looked at, without totals: it changes
    /// as it goes rather than looking at everything first.
    ChangeAccess {
        paths: Vec<PathBuf>,
        change: AccessChange,
    },
    /// Move each of `paths` to the desktop's trash, the one every app
    /// shares, where it can be restored from. Each goes to the trash on
    /// its own drive; nothing is copied across drives.
    Trash { paths: Vec<PathBuf> },
    /// Put each of `items`, entries in a trash as the trash feature lists
    /// them, back where it was trashed from, making the folders it was in
    /// when they are gone. What stands in the way there is asked about as
    /// for a move.
    Restore { items: Vec<PathBuf> },
    /// Delete each of `items`, entries in a trash, for good.
    DeleteFromTrash { items: Vec<PathBuf> },
    /// Delete everything in every trash the user has, for good.
    EmptyTrash,
    /// Undo job `job`, one [`Event::Undoable`] named: copies go to the
    /// trash, moves go back, trashed entries are restored, restored ones
    /// go back to the trash, a rename is renamed back and a new folder
    /// still empty goes. What changed since that job is left alone.
    ///
    /// [`Event::Undoable`]: crate::Event::Undoable
    Undo { job: JobId },
    /// A job one feature defines and runs: the feature's own type, which
    /// only that feature and the features that use its api open.
    Feature(Carried),
}

impl Operation {
    /// A feature's own job `job`.
    pub fn feature<T: Any + Send + Sync + std::fmt::Debug + Eq>(job: T) -> Self {
        Self::Feature(Carried::new(job))
    }

    /// The feature job this is, when it is a `T`.
    pub fn get<T: Any>(&self) -> Option<&T> {
        match self {
            Self::Feature(job) => job.get(),
            _ => None,
        }
    }
}

/// How far a job has got. Totals are known once the job has looked at
/// everything it will do, before it changes anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Progress {
    pub done_bytes: u64,
    pub total_bytes: u64,
    /// Files, folders and links.
    pub done_items: u64,
    pub total_items: u64,
}

/// How a job ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It did everything it was asked to.
    Done,
    /// A client cancelled it. What it finished before stays; nothing is
    /// left half-written.
    Cancelled,
    /// It stopped at `path` for `reason`. What it finished before stays.
    Failed { path: PathBuf, reason: Failure },
}

/// What the user's answers to a job's conflicts made of it, for the
/// note that says how it went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tally {
    /// Files that took the place of one already there.
    pub replaced: u64,
    /// The job's own sources (not what is inside them) left out.
    pub skipped: u64,
}

/// Why a step of a job failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Failure {
    /// Something else already has the name.
    NameTaken,
    /// The name is empty, `.` or `..`, or holds a `/` or a NUL byte, or
    /// a path isn't absolute.
    InvalidName,
    /// The file or folder isn't there any more.
    NotFound,
    /// The user may not change it.
    PermissionDenied,
    /// The drive is full.
    NoSpace,
    /// The drive is mounted read-only.
    ReadOnly,
    /// A folder can't go inside itself.
    IntoItself,
    /// A socket, device or pipe, which a copy can't carry.
    Unsupported,
    /// The file changed while it was copied, so the copy isn't kept.
    SourceChanged,
    /// A Trash item has no record of where it came from.
    NoRecord,
    /// The archive is damaged or cut short.
    Damaged,
    /// The file isn't an archive Nova Files reads.
    NotArchive,
    /// Anything else the system refused.
    Other,
}

/// Something in a job's way that the user decides: the job holds at it
/// until a client answers with one of the answers its kind accepts (see
/// [`ConflictKind::accepts`]), or cancels the job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub kind: ConflictKind,
    /// What the job was copying.
    pub source: PathBuf,
    /// Where it was going.
    pub target: PathBuf,
    /// Which of the job's conflicts it is, counting from 1, so an answer
    /// to an earlier one never settles a later one that looks the same.
    pub number: u64,
    /// How many more of the job's items, as far as the job can see when
    /// it asks, will meet a conflict of the same kind: what an answer for
    /// all of its kind ([`Scope::AllOfThisKind`]) settles beyond this one.
    pub more: u64,
}

/// What stands in a job's way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConflictKind {
    /// A file or link has the name a file or link was going to take.
    FileExists,
    /// A folder has the name a folder was going to take.
    FolderExists,
    /// A folder has the name a file was going to take, or the reverse.
    KindMismatch,
    /// The target is the source itself, or a hard link to it.
    SameFile,
    /// The target's drive is full.
    NoSpace,
    /// The user may not read the source or write the target.
    PermissionDenied,
    /// The source isn't there any more.
    SourceGone,
    /// The source changed while it was copied, so the copy wasn't kept.
    SourceChanged,
    /// A socket, device or pipe, which a copy can't carry.
    Unsupported,
    /// The source's drive has no trash it may use, so it can't go to the
    /// trash; a client may offer to delete it for good instead.
    NoTrash,
    /// The source is in a trash already, so only a delete takes it further.
    InTrash,
    /// A question of the job's own feature, which that feature words and
    /// answers. A client answers it with [`Command::Reply`] or one of the
    /// answers the question takes.
    ///
    /// [`Command::Reply`]: crate::Command::Reply
    Asked(&'static Question),
}

/// A question one feature's jobs ask, beyond the shared ones.
///
/// A feature keeps each of its questions in a `static`; a question is
/// that one `static` and no other, whatever its name says.
pub struct Question {
    name: &'static str,
    takes: u8,
}

impl Question {
    /// A question named `name` in logs, taking `takes` (Skip is always
    /// taken).
    pub const fn new(name: &'static str, takes: &[Answer]) -> Self {
        let mut set = 0;
        let mut at = 0;
        while at < takes.len() {
            set |= takes[at].bit();
            at += 1;
        }
        Self { name, takes: set }
    }

    const fn takes(&self, answer: Answer) -> bool {
        self.takes & answer.bit() != 0
    }
}

impl PartialEq for Question {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Eq for Question {}

impl std::hash::Hash for Question {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::ptr::from_ref(self).hash(state);
    }
}

impl std::fmt::Debug for Question {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name)
    }
}

impl ConflictKind {
    /// Whether a conflict of this kind takes `answer`. Skip always does;
    /// nothing replaces a folder, the same file, or something of another
    /// kind.
    pub const fn accepts(self, answer: Answer) -> bool {
        if let Self::Asked(question) = self {
            return matches!(answer, Answer::Skip) || question.takes(answer);
        }
        match answer {
            Answer::Skip => true,
            Answer::Replace => matches!(self, Self::FileExists),
            Answer::Merge => matches!(self, Self::FolderExists),
            Answer::KeepBoth => matches!(
                self,
                Self::FileExists | Self::FolderExists | Self::KindMismatch | Self::SameFile
            ),
            Answer::Retry => matches!(
                self,
                Self::NoSpace | Self::PermissionDenied | Self::SourceChanged | Self::NoTrash
            ),
        }
    }
}

/// What the user chose for a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Answer {
    /// Leave this one out (a folder with everything in it) and go on.
    Skip,
    /// Put the source in the target's place. The target is removed only
    /// once its replacement is whole.
    Replace,
    /// Copy the folder's contents into the folder already there; each
    /// clash inside is asked about on its own.
    Merge,
    /// Give the copy the next free name, "name (2).ext".
    KeepBoth,
    /// Try the same step again, once space is freed or rights fixed.
    Retry,
}

impl Answer {
    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// Which conflicts an answer covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Only the one asked about.
    ThisOne,
    /// It and every later conflict of the same kind in this job.
    AllOfThisKind,
}

#[cfg(test)]
mod tests;
