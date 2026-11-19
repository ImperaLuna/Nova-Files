//! Thumbnails: small pictures of a listing's files, drawn in place of
//! their icons.
//!
//! A client asks for them with [`nova_files_api::Command::Rows`]; the core
//! answers each with [`Event::Thumbnail`].

#![forbid(unsafe_code)]

use std::sync::Arc;

use nova_files_api::{EntryId, ListingId, ThumbnailSize};

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
}
