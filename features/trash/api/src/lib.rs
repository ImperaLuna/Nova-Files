//! The trash: its jobs, the questions they ask, and what the trash
//! feature's two halves say to each other.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::time::SystemTime;

use nova_files_api::{Answer, EntryKind, Operation, Question};

/// Move each of `paths` to the desktop's trash, the one every app shares.
///
/// Each goes to the trash on its own drive, where it can be restored
/// from; nothing is copied across drives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashJob {
    pub paths: Vec<PathBuf>,
}

/// Put each of `items`, entries in a trash, back where it was trashed from.
///
/// The folders it was in are made again when they are gone. What stands
/// in the way there is asked about as for a move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreJob {
    pub items: Vec<PathBuf>,
}

/// Delete each of `items`, entries in a trash, for good.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteFromTrashJob {
    pub items: Vec<PathBuf>,
}

/// Delete everything in every trash the user has, for good.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmptyTrashJob;

/// The job that moves `paths` to the trash.
pub fn trash(paths: Vec<PathBuf>) -> Operation {
    Operation::feature(TrashJob { paths })
}

/// The job that puts `items` back where they were trashed from.
pub fn restore(items: Vec<PathBuf>) -> Operation {
    Operation::feature(RestoreJob { items })
}

/// The job that deletes `items`, entries in a trash, for good.
pub fn delete_from_trash(items: Vec<PathBuf>) -> Operation {
    Operation::feature(DeleteFromTrashJob { items })
}

/// The job that empties every trash.
pub fn empty_trash() -> Operation {
    Operation::feature(EmptyTrashJob)
}

/// Asked when an item's drive has no trash it may use, so it can't go to
/// the trash. Retry tries again; a client may offer to delete it for good
/// instead.
pub static NO_TRASH: Question = Question::new("no-trash", &[Answer::Retry]);

/// Asked when an item is in a trash already, so only a delete takes it
/// further.
pub static IN_TRASH: Question = Question::new("in-trash", &[]);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trashed {
    pub item: PathBuf,
    pub original: PathBuf,
    pub deleted: Option<SystemTime>,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    List,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Listed {
        folder: Option<PathBuf>,
        items: Vec<Trashed>,
        unrecorded: usize,
    },
}
