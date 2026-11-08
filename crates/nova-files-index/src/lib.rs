//! The file index: one memory-mapped file of names and metadata, plus its
//! change log.
//!
//! [`crawl`] walks a folder (the user's home) depth first, choosing what to
//! index with the rules in `policy`, and returns a [`Built`] index;
//! [`Built::save`] writes it next to the old one and renames it over it, so
//! a reader never sees half a file. [`Index::open`] maps a saved index and
//! checks it once, so every lookup after that is plain reads of the map.
//! The layout is in `format`.
//!
//! The index is a rebuildable cache, not a database: an index that is
//! missing, out of date or damaged is crawled again.

mod build;
mod crawl;
mod format;
mod policy;
mod reader;

pub use build::Built;
pub use crawl::{CrawlError, crawl};
pub use reader::{Index, OpenError};

/// A row's position in the index. Row 0 is the crawled folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RowId(pub u32);

/// What stays the same for a file while it is renamed or moved within its
/// drive: the device and inode numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId {
    pub device: u64,
    pub inode: u64,
}

/// What a row is. Links are never followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Folder,
    File,
    Link,
}

/// A time, in nanoseconds since the Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Stamp(pub i64);

/// Facts about a row that aren't its kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Flags(pub u8);

impl Flags {
    /// The folder, or the entry's details, couldn't be read; a folder with
    /// this flag has no rows under it.
    pub const UNREADABLE: Self = Self(1);
    /// A folder on another drive than the crawled folder, mounted inside
    /// it. It isn't entered.
    pub const OTHER_DRIVE: Self = Self(1 << 1);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// One indexed entry, as [`Index::entry`] reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// `None` for row 0.
    pub parent: Option<RowId>,
    /// One past the last row under this one: rows from this one up to
    /// `end` are it and everything inside it. For anything but a folder,
    /// the next row.
    pub end: RowId,
    pub kind: Kind,
    pub flags: Flags,
    pub id: FileId,
    /// In bytes, as the filesystem reports it.
    pub size: u64,
    pub modified: Option<Stamp>,
    /// Where the filesystem records it.
    pub created: Option<Stamp>,
}
