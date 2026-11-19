//! Finding files by name, as the core reports it.

#![forbid(unsafe_code)]

use std::path::PathBuf;

use nova_files_api::ListingId;

/// The most hits one search stage may hold: more than a launcher shows.
/// The core keeps the best ones.
pub const MAX_HITS: u32 = 200;

/// The most entries each stage of a [`Command::Find`] adds: more than a
/// window's list scrolls through when looking for one name.
pub const MAX_FOUND: u32 = 1000;

/// A request from a client to the search feature's core half.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Find files under home whose names match `query`, one word or
    /// several separated by spaces. The answer is an [`Event::Found`] for
    /// each stage, best first, at most `limit` hits each (and at most
    /// [`MAX_HITS`]), or [`Event::SearchFailed`]. The first search builds
    /// or opens the home index, so it may take a while; later ones take
    /// milliseconds. A newer `Search` from the same client cancels this
    /// one, which then may stop answering.
    Search {
        id: SearchId,
        query: String,
        limit: u32,
    },
    /// Find files whose names match `query`, as [`Command::Search`] does,
    /// for a window to show as its list. The answer is
    /// [`Event::FolderListed`](nova_files_api::Event::FolderListed) with
    /// `id` and every stage's hits, best first, each entry in its own
    /// folder (see [`Listing::folders`](nova_files_api::Listing::folders)),
    /// at most [`MAX_FOUND`] a stage; or [`Event::FindFailed`]. `within`
    /// keeps only what is inside that folder, at any depth; without it the
    /// whole index counts. It is the client's listing like a
    /// [`Command::ListFolder`](nova_files_api::Command::ListFolder): either
    /// one asked for later cancels it, and
    /// [`Command::Rows`](nova_files_api::Command::Rows) serves its rows.
    Find {
        id: ListingId,
        query: String,
        within: Option<PathBuf>,
    },
}

/// Something the search feature's core half tells one client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// One stage of [`Command::Search`] `id`: names as typed come first,
    /// then letters in order and typos. `done` marks the last stage. A path
    /// shows at most once per search.
    Found {
        id: SearchId,
        hits: Vec<Hit>,
        done: bool,
    },
    /// [`Command::Search`] `id` couldn't run.
    SearchFailed { id: SearchId, reason: SearchFailed },
    /// [`Command::Find`] `id` couldn't run.
    FindFailed { id: ListingId, reason: SearchFailed },
}

/// Names one search request.
///
/// The client picks it and the core echoes it in each answer. A newer
/// search from the same client makes the core give up on the older one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SearchId(pub u64);

/// How a name matched the query, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Matched {
    /// The whole name, or the name without its extension.
    Exact,
    /// At the start of a word in the name.
    WordStart,
    /// Anywhere in the name.
    Contains,
    /// The query's letters appear in the name in order.
    InOrder,
    /// With one letter missing, replaced or swapped.
    OneTypo,
}

/// One search result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub path: PathBuf,
    pub matched: Matched,
}

/// Why the core couldn't search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SearchFailed {
    /// There is no home folder or cache folder to index.
    NoIndex,
    /// The home folder couldn't be indexed.
    IndexFailed,
}
