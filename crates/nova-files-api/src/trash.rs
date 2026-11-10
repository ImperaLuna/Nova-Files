//! What the core says about the desktop's trash: each entry in it,
//! answering [`Command::ListTrash`].
//!
//! An entry is named by [`Trashed::item`], where it sits in its trash;
//! that is what [`Operation::Restore`] and [`Operation::DeleteFromTrash`]
//! take.
//!
//! [`Command::ListTrash`]: crate::Command::ListTrash
//! [`Operation::Restore`]: crate::Operation::Restore
//! [`Operation::DeleteFromTrash`]: crate::Operation::DeleteFromTrash

use std::path::PathBuf;
use std::time::SystemTime;

use crate::folder::EntryKind;

/// One entry in a trash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trashed {
    /// Where it sits in its trash.
    pub item: PathBuf,
    /// Where it was when trashed, and where a restore puts it back.
    pub original: PathBuf,
    /// When it was trashed; `None` when its info file doesn't say so in
    /// a way that can be read.
    pub deleted: Option<SystemTime>,
    pub kind: EntryKind,
}
