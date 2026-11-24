//! Copying and moving files, and dropping them on a folder: what a drop
//! would do, and doing it.
//!
//! A copy or a move is a job: [`CopyJob`] and [`MoveJob`], started with
//! [`nova_files_api::Command::StartJob`] as [`Operation::feature`] of one.
//!
//! A drop copies or moves its files as a job, like
//! [`nova_files_api::Command::StartJob`]: the answer is
//! [`nova_files_api::Event::JobStarted`] with the same tag, and the job
//! then runs as any other.

#![forbid(unsafe_code)]

use std::path::PathBuf;

use nova_files_api::{JobTag, Operation};

/// Copy each of `sources` into the folder `into`, under its own name.
///
/// Files, folders with all they hold, and links as links. Each file is
/// written under a hidden name and only takes its own once whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyJob {
    pub sources: Vec<PathBuf>,
    pub into: PathBuf,
}

/// Move each of `sources` into the folder `into`, under its own name.
///
/// On one drive each is renamed; across drives it is copied as by
/// [`CopyJob`], and each source item goes only once its copy is whole,
/// folders last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveJob {
    pub sources: Vec<PathBuf>,
    pub into: PathBuf,
}

/// Make a symbolic link in the folder `into` to each of `sources`, under
/// the source's name, or `Link to <name>` beside the source itself. A name
/// already taken gets a keep-both number; nothing is replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkJob {
    pub sources: Vec<PathBuf>,
    pub into: PathBuf,
}

/// The job that links `sources` into `into`.
pub fn link(sources: Vec<PathBuf>, into: PathBuf) -> Operation {
    Operation::feature(LinkJob { sources, into })
}

/// The job that copies `sources` into `into`.
pub fn copy(sources: Vec<PathBuf>, into: PathBuf) -> Operation {
    Operation::feature(CopyJob { sources, into })
}

/// The job that moves `sources` into `into`.
pub fn move_into(sources: Vec<PathBuf>, into: PathBuf) -> Operation {
    Operation::feature(MoveJob { sources, into })
}

/// The words a copy and a move open their journal with, so a job left
/// by a crash is known again.
pub mod journal {
    pub const COPY: &str = "copy";
    pub const MOVE: &str = "move";
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Copy or move `sources` into the folder `into`, as dragging them
    /// there does: moved when every one is on `into`'s drive, copied
    /// otherwise. The core picks; [`nova_files_api::Event::JobStarted`]
    /// says which, and the job then runs as a
    /// [`nova_files_api::Command::StartJob`] of it would.
    Drop {
        tag: JobTag,
        sources: Vec<PathBuf>,
        into: PathBuf,
    },
    /// Say what [`Command::Drop`] of `sources` into `into` would do,
    /// without doing it: for a drag to name its drop before it lands. The
    /// answer is [`Event::DropWould`] with the same `id`.
    DropWould {
        id: DropAskId,
        sources: Vec<PathBuf>,
        into: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// [`Command::DropWould`] `id`'s drop would `kind` its files. Only the
    /// client that asked hears it.
    DropWould { id: DropAskId, kind: DropKind },
}

/// Names one ask of what a drop would do. The client picks it; the core
/// only echoes it, so a client that asked twice knows which answer is
/// which.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DropAskId(pub u64);

/// What dropping files on a folder does: a move when every file is on
/// the folder's drive, a copy otherwise. Only a key held picks a link;
/// [`Event::DropWould`] never answers one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropKind {
    Move,
    Copy,
    Link,
}
