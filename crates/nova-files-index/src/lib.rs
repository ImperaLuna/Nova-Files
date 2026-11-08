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
//! While Nova Files runs, a [`Watcher`] follows every indexed folder with
//! inotify and appends what changes to a [`ChangeLog`], which any number
//! of [`LogReader`]s follow.
//!
//! [`Index::search`] finds rows by name, in stages ([`Search`]);
//! [`Index::search_now`] does the same with the changes since the crawl,
//! kept in an [`Overlay`].
//!
//! The index is a rebuildable cache, not a database: an index that is
//! missing, out of date or damaged is crawled again.

mod build;
mod changes;
mod crawl;
mod format;
mod mounts;
mod overlay;
mod policy;
mod reader;
mod search;
mod watch;

pub use build::Built;
pub use changes::{Change, ChangeLog, LogReader};
pub use crawl::{CrawlError, crawl};
pub use overlay::Overlay;
pub use reader::{Index, OpenError};
pub use search::{Hit, MatchKind, STAGES, Search};
pub use watch::Watcher;

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

/// Details about a row that aren't its kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Flags(pub u8);

impl Flags {
    /// The folder, or the entry's details, couldn't be read; a folder with
    /// this flag has no rows under it.
    pub const UNREADABLE: Self = Self(1);
    /// A folder something is mounted on: another drive, a network share,
    /// a bind mount. It isn't entered.
    pub const OTHER_DRIVE: Self = Self(1 << 1);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// What is known about one entry, apart from its name and place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Details {
    pub kind: Kind,
    pub flags: Flags,
    pub id: FileId,
    /// In bytes, as the filesystem reports it.
    pub size: u64,
    pub modified: Option<Stamp>,
    /// Where the filesystem records it.
    pub created: Option<Stamp>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_combine_and_contain() {
        let both = Flags::UNREADABLE.with(Flags::OTHER_DRIVE);
        assert!(both.contains(Flags::UNREADABLE));
        assert!(both.contains(Flags::OTHER_DRIVE));
        assert!(!Flags::UNREADABLE.contains(Flags::OTHER_DRIVE));
        assert!(!Flags::default().contains(Flags::UNREADABLE));
        assert_eq!(Flags::UNREADABLE.with(Flags::UNREADABLE), Flags::UNREADABLE);
    }
}
