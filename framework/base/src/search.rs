//! Finding files by name, as the core reports it.

use std::path::PathBuf;

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
