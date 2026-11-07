//! What clients say to the core and what the core says back.

use std::path::PathBuf;
use std::sync::Arc;

use crate::folder::{Details, EntryId, Listing, ListingId, Place, Unreadable};

/// A request from a client to the core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Ask the core to answer with [`Event::Pong`], to show it is running.
    Ping,
    /// List the folder at `path`. The answer is [`Event::FolderListed`] or
    /// [`Event::FolderUnreadable`] with the same `id`; a listing is followed
    /// by [`Event::DetailsRead`] batches. A newer `ListFolder` from the same
    /// client cancels this one, which then may never answer.
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
    /// Details for entries `first` onward of listing `id`, one per entry,
    /// in entry order. Batches follow [`Event::FolderListed`] in order, with
    /// no gaps, until every entry has its details or a newer listing
    /// cancels them.
    DetailsRead {
        id: ListingId,
        first: EntryId,
        details: Arc<[Details]>,
    },
    /// The standard folders that exist, in places-list order.
    Places(Vec<Place>),
}

/// Names one connected client, so the core can answer the right one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClientId(pub u64);
