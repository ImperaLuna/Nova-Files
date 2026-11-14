//! What a quick look shows of a file or folder: its contents, large, with
//! the name, kind and size beside them.

use std::fmt;
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

/// The longest side a [`Fit`] counts; a bigger one is taken as this.
pub const MAX_FIT_SIDE: u32 = 8192;

/// The box a picture is scaled down to fit, in device pixels: what the
/// client has room to draw. A smaller picture keeps its size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fit {
    pub width: u32,
    pub height: u32,
}

impl Fit {
    /// This box with each side at most [`MAX_FIT_SIDE`] and at least 1.
    #[must_use]
    pub fn clamped(self) -> Self {
        Self {
            width: self.width.clamp(1, MAX_FIT_SIDE),
            height: self.height.clamp(1, MAX_FIT_SIDE),
        }
    }
}

/// A picture, decoded and scaled to fit.
#[derive(Clone, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    /// RGBA, four bytes a pixel, row by row from the top.
    pub pixels: Arc<[u8]>,
}

/// Only the size: the pixels would fill a test's failure message.
impl fmt::Debug for Picture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Picture")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

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
    /// A picture the core could decode, upright, scaled down to the
    /// request's [`Fit`].
    Picture(Picture),
    /// The start of a text file, at most [`PREVIEW_TEXT_BYTES`]; `cut`
    /// when the file goes on past it.
    Text { text: Arc<str>, cut: bool },
    /// A folder's first entries, folders first and A to Z, at most
    /// [`PREVIEW_ENTRIES`], and how many it holds in all.
    Folder { first: Vec<Entry>, count: usize },
    /// Nothing the preview can draw: an empty or binary file, a picture
    /// that wouldn't decode, or a device, pipe or socket. The client shows
    /// its icon.
    Icon,
}
