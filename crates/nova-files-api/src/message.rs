//! What clients say to the core and what the core says back.

use std::path::PathBuf;

use crate::folder::{Listing, ListingId, Place, Unreadable};

/// A request from a client to the core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Ask the core to answer with [`Event::Pong`], to show it is running.
    Ping,
    /// List the folder at `path`. The answer is [`Event::FolderListed`] or
    /// [`Event::FolderUnreadable`] with the same `id`. A newer `ListFolder`
    /// from the same client cancels this one, which then may never answer.
    ListFolder { id: ListingId, path: PathBuf },
    /// Ask which standard folders exist; the answer is [`Event::Places`].
    Places,
}

/// Something the core tells one client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The answer to [`Command::Ping`].
    Pong,
    /// A folder's entries, answering [`Command::ListFolder`].
    FolderListed(Listing),
    /// The folder of [`Command::ListFolder`] `id` couldn't be read.
    FolderUnreadable {
        id: ListingId,
        path: PathBuf,
        reason: Unreadable,
    },
    /// The standard folders that exist, in places-list order.
    Places(Vec<Place>),
}

/// Names one connected client, so the core can answer the right one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClientId(pub u64);
