//! Reading each entry's size and modified time, after its folder is listed.
//!
//! The listing goes out first, so the names show at once; the details
//! follow in batches of [`BATCH`] entries, in entry order. [`read_batch`]
//! asks `cancelled` before each read from the disk and gives up as soon as
//! it says so. A link reports what it points to; a broken link reports
//! itself. An entry that can't be read (it vanished, say) gets empty
//! details instead of stopping the rest.

use std::fs;
use std::path::Path;

use nova_files_api::{Details, Entry, EntryKind};

/// Entries per [`nova_files_api::Event::DetailsRead`]: large enough that a
/// 100,000-entry folder sends few events, small enough that the first
/// screen fills in quickly.
pub const BATCH: usize = 2048;

/// The details of each of `entries`, all in the folder at `folder`, in the
/// same order. `None` when `cancelled` said so before the batch was done.
pub fn read_batch(
    folder: &Path,
    entries: &[Entry],
    cancelled: impl Fn() -> bool,
) -> Option<Vec<Details>> {
    let mut details = Vec::with_capacity(entries.len());
    for entry in entries {
        details.push(read_one(&folder.join(&entry.name), entry.kind, &cancelled)?);
    }
    Some(details)
}

/// One entry's details; `None` when `cancelled` said so before a read.
fn read_one(path: &Path, kind: EntryKind, cancelled: impl Fn() -> bool) -> Option<Details> {
    if cancelled() {
        return None;
    }
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        // Maybe a broken link, which reports itself.
        Err(_) if cancelled() => return None,
        Err(_) => match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(_) => return Some(Details::default()),
        },
    };
    Some(Details {
        size: (kind == EntryKind::File).then_some(metadata.len()),
        modified: metadata.modified().ok(),
    })
}
