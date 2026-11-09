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
