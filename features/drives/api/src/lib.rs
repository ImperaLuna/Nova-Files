//! The drives and volumes the core watches, how full a drive is, and the
//! commands and events a client uses to hear about them.

#![forbid(unsafe_code)]

mod disk_space;
mod drive;

use std::path::PathBuf;
use std::sync::Arc;

pub use disk_space::{DiskSpace, MAX_DISK_SPACE_PATHS, PathSpace};
pub use drive::{
    Drive, DriveAction, DriveFailure, DriveId, DriveList, DriveOffer, DriveService, Volume,
    VolumeId,
};

/// A request from a client about drives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Ask for the drives and volumes; the answer is [`Event::Drives`],
    /// and from then on the client gets [`Event::Drives`] again each time
    /// one comes, goes or changes. Clients that never ask never get them.
    Drives,
    /// Mount, unmount or eject. The answer, once it is done, is
    /// [`Event::DriveDone`]; the change itself reaches every client that
    /// asked for the drives as [`Event::Drives`].
    Drive(DriveAction),
    /// Ask how full the drives holding `paths` (absolute paths) are. The
    /// answer is [`Event::DiskSpace`], one entry per path in the same
    /// order; paths past [`MAX_DISK_SPACE_PATHS`] are left out.
    DiskSpace(Vec<PathBuf>),
}

/// Something the core tells one client about drives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The drives and volumes, answering [`Command::Drives`], and sent
    /// again to each client that asked whenever one comes, goes or
    /// changes.
    Drives(Arc<DriveList>),
    /// [`Command::Drive`] `action` is done: for a mount, with where the
    /// volume is now. Only the client that asked hears it.
    DriveDone {
        action: DriveAction,
        result: Result<Option<PathBuf>, DriveFailure>,
    },
    /// How full the drives are, answering [`Command::DiskSpace`]: one
    /// entry per path asked, in the same order.
    DiskSpace(Vec<PathSpace>),
}
