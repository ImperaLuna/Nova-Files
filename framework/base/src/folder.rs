//! What a folder holds, as the core reports it: its entries, each entry's
//! details, and why a folder couldn't be read.
//!
//! A listing is shared, not copied: the core hands one `Arc` to the window,
//! however many entries it has.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
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

impl Entry {
    /// Whether the entry is hidden: its name starts with a dot.
    pub fn is_hidden(&self) -> bool {
        self.name.as_encoded_bytes().first() == Some(&HIDDEN_MARK)
    }
}

/// The first byte of a hidden entry's name.
const HIDDEN_MARK: u8 = b'.';

/// What the core reads about one entry after listing it. A field is `None`
/// when the disk couldn't say (the entry vanished, or can't be read).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Details {
    /// The file's size in bytes. Always `None` for a folder.
    pub size: Option<u64>,
    /// When the contents last changed.
    pub modified: Option<SystemTime>,
}

/// What a listing's entries are ordered by. Folders always come before
/// files; within each group the entries follow the key, and entries the
/// key can't tell apart (folders by size, say) stay A to Z.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SortKey {
    #[default]
    Name,
    Modified,
    Size,
}

/// How a client wants a folder's entries ordered: by `key`, smallest
/// first (A to Z, oldest, smallest) unless `descending`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Order {
    pub key: SortKey,
    pub descending: bool,
}

/// A folder's entries, folders first, in the [`Order`] the client asked
/// for; or the files a search found, best first, each in its own folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub id: ListingId,
    /// The folder listed, or the folder a search looked in; empty for a
    /// gathered place (Games, Projects).
    pub path: PathBuf,
    pub entries: Arc<[Entry]>,
    /// For a search, the folder each entry is in, by entry; `None` when
    /// every entry is in `path`.
    pub folders: Option<Arc<[PathBuf]>>,
}

impl Listing {
    /// The folder entry `index` is in: its own for a search, otherwise
    /// [`Listing::path`].
    pub fn folder_of(&self, index: usize) -> &Path {
        self.folders
            .as_ref()
            .and_then(|folders| folders.get(index))
            .unwrap_or(&self.path)
    }

    /// Where `entry`, one of this listing's, is on disk.
    pub fn path_of(&self, entry: &Entry) -> PathBuf {
        let index = usize::try_from(entry.id.0).unwrap_or(usize::MAX);
        self.folder_of(index).join(&entry.name)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u32, name: &str) -> Entry {
        Entry {
            id: EntryId(id),
            name: name.into(),
            kind: EntryKind::File,
        }
    }

    #[test]
    fn an_entry_is_in_its_own_folder_or_the_listing_s() {
        let folder = Listing {
            id: ListingId(1),
            path: PathBuf::from("/home"),
            entries: Arc::new([]),
            folders: None,
        };
        assert_eq!(folder.path_of(&entry(0, "a.txt")), Path::new("/home/a.txt"));
        let found = Listing {
            folders: Some(Arc::new([PathBuf::from("/docs")])),
            ..folder
        };
        assert_eq!(found.path_of(&entry(0, "a.txt")), Path::new("/docs/a.txt"));
    }

    #[test]
    fn a_name_starting_with_a_dot_is_hidden() {
        assert!(entry(0, ".config").is_hidden());
        assert!(entry(0, "..").is_hidden());
        assert!(!entry(0, "notes.txt").is_hidden());
        assert!(!entry(0, "").is_hidden());
        let raw = Entry {
            name: std::os::unix::ffi::OsStringExt::from_vec(vec![b'.', 0xff]),
            ..entry(0, "")
        };
        assert!(raw.is_hidden());
    }
}
