//! The folders a person opened lately, for the Start page.
//!
//! A window tells the core each folder a person opens with
//! [`Command::FolderUsed`]; the core keeps the newest
//! [`MAX_RECENT_FOLDERS`] between runs. A client asks for them with
//! [`Command::RecentFolders`] and hears [`Event::RecentFolders`]: the
//! newest first, each still a folder that can be read, with a peek at its
//! first few entries.
//!
//! [`Command::FolderUsed`]: crate::Command::FolderUsed
//! [`Command::RecentFolders`]: crate::Command::RecentFolders
//! [`Event::RecentFolders`]: crate::Event::RecentFolders

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::SystemTime;

use crate::folder::EntryKind;

/// The most folders the core remembers.
pub const MAX_RECENT_FOLDERS: usize = 50;

/// The most entries a [`RecentFolder`] peeks at.
pub const PEEK_ENTRIES: usize = 5;

/// A folder opened lately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentFolder {
    pub path: PathBuf,
    /// When its contents last changed, if the disk could say.
    pub modified: Option<SystemTime>,
    /// How many entries it holds.
    pub entries: u64,
    /// Its first entries in the listing's order (folders first, each group
    /// A to Z), at most [`PEEK_ENTRIES`].
    pub peek: Vec<Peeked>,
}

/// One entry of a [`RecentFolder`]'s peek.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peeked {
    /// The name exactly as the disk has it, which may not be valid UTF-8.
    pub name: OsString,
    pub kind: EntryKind,
    /// A file's size in bytes, or how many entries a folder holds; `None`
    /// when the disk couldn't say.
    pub amount: Option<u64>,
}
