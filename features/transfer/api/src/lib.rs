//! Dropping files on a folder: what a drop would do, and doing it.
//!
//! A drop copies or moves its files as a job, like
//! [`nova_files_api::Command::StartJob`]: the answer is
//! [`nova_files_api::Event::JobStarted`] with the same tag, and the job
//! then runs as any other.

#![forbid(unsafe_code)]

use std::path::PathBuf;

use nova_files_api::JobTag;

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

/// What dropping files on a folder does when no key picks: a move when
/// every file is on the folder's drive, a copy otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropKind {
    Move,
    Copy,
}
