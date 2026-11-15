//! What a quick look shows of a file or folder: its contents, large, with
//! the name, kind and size beside them.

use std::path::PathBuf;
use std::sync::Arc;

use crate::folder::Entry;

/// Names one preview request.
///
/// The client picks it and the core echoes it in the answer. A newer
/// preview from the same client makes the core give up on the older one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PreviewId(pub u64);

/// The most entries a folder's preview names; the rest are only counted.
pub const PREVIEW_ENTRIES: usize = 9;

/// The most bytes of a text file a preview holds: more than a window
/// shows.
pub const PREVIEW_TEXT_BYTES: usize = 32 * 1024;

/// A file or folder as a quick look shows it, answering
/// [`Command::Preview`](crate::Command::Preview).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    pub path: PathBuf,
    /// What its type is called in the user's MIME database ("PNG image"),
    /// from its name; `None` for a folder, or a name the database doesn't
    /// know.
    pub kind_name: Option<String>,
    /// Its length in bytes. Always 0 for a folder.
    pub bytes: u64,
    pub content: PreviewContent,
}

/// What a preview shows large.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewContent {
    /// The start of a text file, at most [`PREVIEW_TEXT_BYTES`]; `cut`
    /// when the file goes on past it.
    Text { text: Arc<str>, cut: bool },
    /// A folder's first entries, folders first and A to Z, at most
    /// [`PREVIEW_ENTRIES`], and how many it holds in all.
    Folder { first: Vec<Entry>, count: usize },
    /// Nothing the preview can draw: an empty or binary file, or a device,
    /// pipe or socket. The client shows its icon.
    Icon,
}
