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

use std::ffi::OsString;
use std::path::PathBuf;

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
    /// Move each of `paths` to the desktop's trash, the one every app
    /// shares, where it can be restored from. Each goes to the trash on
    /// its own drive; nothing is copied across drives.
    Trash { paths: Vec<PathBuf> },
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
}

impl ConflictKind {
    /// Whether a conflict of this kind takes `answer`. Skip always does;
    /// nothing replaces a folder, the same file, or something of another
    /// kind.
    pub const fn accepts(self, answer: Answer) -> bool {
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

/// Which conflicts an answer covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Only the one asked about.
    ThisOne,
    /// It and every later conflict of the same kind in this job.
    AllOfThisKind,
}
