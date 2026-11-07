//! What a folder holds, as the core reports it: its entries, each entry's
//! details, and why a folder couldn't be read.
//!
//! A listing is shared, not copied: the core hands one `Arc` to the window,
//! however many entries it has.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

/// Names one listing request.
///
/// The client picks it, the core echoes it in the answer, and the client
/// drops any answer that isn't for its newest request. Asking again makes
/// the core give up on the older request. `u64::MAX` is reserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ListingId(pub u64);

/// Names one entry within its listing: the entry's position in
/// [`Listing::entries`]. It is only meaningful together with that listing's
/// [`ListingId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntryId(pub u32);

/// Whether an entry is a folder (something you can go into) or anything
/// else. A link counts as what it points to; a broken link is a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryKind {
    Folder,
    File,
}

/// One thing in a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: EntryId,
    /// The name exactly as the disk has it, which may not be valid UTF-8.
    pub name: OsString,
    pub kind: EntryKind,
}

/// What the core reads about one entry after listing it. A field is `None`
/// when the disk couldn't say (the entry vanished, or can't be read).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Details {
    /// The file's size in bytes. Always `None` for a folder.
    pub size: Option<u64>,
    /// When the contents last changed.
    pub modified: Option<SystemTime>,
}

/// A folder's entries: folders first, then files, each group A to Z.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub id: ListingId,
    pub path: PathBuf,
    pub entries: Arc<[Entry]>,
}

/// Why a folder couldn't be listed. Normal cases, not bugs: a client
/// decides what to show for each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Unreadable {
    /// Nothing is at the path (any more).
    Missing,
    /// The user may not read it.
    PermissionDenied,
    /// The path is not a folder.
    NotAFolder,
    /// Any other error from the disk.
    Failed,
}

/// A standard folder shown in the places list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PlaceKind {
    Home,
    Documents,
    Pictures,
    Downloads,
}

/// A standard folder that exists on this system, and where it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub kind: PlaceKind,
    pub path: PathBuf,
}
