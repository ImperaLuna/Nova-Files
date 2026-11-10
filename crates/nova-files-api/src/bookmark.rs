//! Pinned folders: the folders a user pins to the places list, each with
//! an optional label of their own.
//!
//! The core keeps them in the bookmarks file other file managers share, so
//! a folder pinned here shows up there too, and back. A client asks for
//! them with [`Command::Places`] and changes them with
//! [`Command::ChangeBookmark`]; every client that asked hears
//! [`Event::Bookmarks`] after each change, from any client or the file.
//!
//! [`Command::Places`]: crate::Command::Places
//! [`Command::ChangeBookmark`]: crate::Command::ChangeBookmark
//! [`Event::Bookmarks`]: crate::Event::Bookmarks

use std::path::PathBuf;

/// A pinned folder. The folder may be missing (on a drive not plugged in);
/// it stays pinned until the user unpins it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Bookmark {
    pub path: PathBuf,
    /// The name the user gave it; without one it shows the folder's name.
    pub label: Option<String>,
}

/// One change to the pinned folders. A change naming a folder that isn't
/// pinned (another window unpinned it first) does nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookmarkChange {
    /// Pin the folder at `path`, labelled `label`, before the pinned
    /// folder now at position `at`, or last when `at` is `None` or past the
    /// end. A folder already pinned stays where it is.
    Add {
        path: PathBuf,
        label: Option<String>,
        at: Option<u32>,
    },
    /// Unpin the folder at `path`.
    Remove { path: PathBuf },
    /// Give the pinned folder at `path` the label `label`, or take its
    /// label off with `None`.
    Rename {
        path: PathBuf,
        label: Option<String>,
    },
    /// Move the pinned folder at `path` to position `to`, or last when
    /// `to` is past the end.
    Move { path: PathBuf, to: u32 },
}
