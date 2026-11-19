//! The places list: the standard folders that exist on this system and the
//! folders a user pins, and the commands and events a client uses for
//! them.

#![forbid(unsafe_code)]

mod bookmark;
mod place;

use std::sync::Arc;

pub use bookmark::{Bookmark, BookmarkChange};
pub use place::{Place, PlaceKind};

/// A request from a client about the places list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Ask for the places list: the answer is [`Event::Places`] and
    /// [`Event::Bookmarks`], and from then on the client gets each again
    /// whenever it changes (a folder made or removed, the bookmarks file
    /// edited). Clients that never ask never get them.
    Places,
    /// Pin, unpin, rename or move a pinned folder and save it. Every client
    /// that asked for the places, this one included, then gets
    /// [`Event::Bookmarks`]; nothing is sent when nothing changed.
    ChangeBookmark(BookmarkChange),
}

/// Something the core tells one client about the places list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The standard folders that exist, in places-list order.
    Places(Vec<Place>),
    /// The pinned folders, in the user's order.
    Bookmarks(Arc<[Bookmark]>),
}

/// What a window knows of the places list: the standard folders and the
/// pinned ones, as last told.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Known {
    /// The standard folders that exist, in places-list order.
    pub places: Vec<Place>,
    /// The pinned folders, in the user's order.
    pub pinned: Arc<[Bookmark]>,
}
