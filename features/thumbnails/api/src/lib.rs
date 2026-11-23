//! Thumbnails: small pictures of a listing's files, drawn in place of
//! their icons, and a peek at what each folder of the listing holds.
//!
//! A client asks for them with [`nova_files_api::Command::Rows`]; the core
//! answers each file with [`Event::Thumbnail`] and each folder with
//! [`Event::Peek`].

#![forbid(unsafe_code)]

use std::ffi::OsString;
use std::sync::Arc;

use nova_files_api::{EntryId, EntryKind, ListingId, ThumbnailSize};

/// The most entries one [`Event::Peek`] names.
pub const PEEKED: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A thumbnail for entry `entry` of listing `id`, as PNG bytes that fit
    /// in a square `size` across.
    Thumbnail {
        id: ListingId,
        entry: EntryId,
        size: ThumbnailSize,
        png: Arc<[u8]>,
    },
    /// What folder `entry` of listing `id` holds, at most [`PEEKED`] of its
    /// entries: pictures and videos first, then other files, then folders,
    /// each run A to Z. Hidden entries come only when nothing else is
    /// there. Empty when the folder is. A folder that can't be read gets
    /// no peek.
    Peek {
        id: ListingId,
        entry: EntryId,
        size: ThumbnailSize,
        peeked: Vec<Peeked>,
    },
}

/// One entry of an [`Event::Peek`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peeked {
    /// The name exactly as the disk has it, which may not be valid UTF-8.
    pub name: OsString,
    pub kind: EntryKind,
    /// A picture's or video's thumbnail, as PNG bytes that fit in a square
    /// the peek's `size` across; `None` for anything else or when none
    /// could be made.
    pub png: Option<Arc<[u8]>>,
}

const HIDDEN_MARK: u8 = b'.';

impl Peeked {
    /// Whether the entry is hidden: its name starts with a dot.
    #[must_use]
    pub fn is_hidden(&self) -> bool {
        self.name.as_encoded_bytes().first() == Some(&HIDDEN_MARK)
    }
}
