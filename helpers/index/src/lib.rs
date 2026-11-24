mod build;
mod changes;
mod crawl;
mod fold;
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
pub use mounts::local_mount_points;
pub use overlay::Overlay;
pub use reader::{Index, OpenError};
pub use search::{Hit, Kept, MatchKind, STAGES, Search};
pub use watch::Watcher;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RowId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId {
    pub device: u64,
    pub inode: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Folder,
    File,
    Link,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Stamp(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Flags(pub u8);

impl Flags {
    pub const UNREADABLE: Self = Self(1);
    pub const OTHER_DRIVE: Self = Self(1 << 1);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Details {
    pub kind: Kind,
    pub flags: Flags,
    pub id: FileId,
    pub size: u64,
    pub modified: Option<Stamp>,
    pub created: Option<Stamp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub parent: Option<RowId>,
    pub end: RowId,
    pub kind: Kind,
    pub flags: Flags,
    pub id: FileId,
    pub size: u64,
    pub modified: Option<Stamp>,
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
