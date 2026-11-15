//! Reading entries' size and modified time, after their folder is listed.
//!
//! The listing goes out first, so the names show at once; the details of
//! the entries on screen follow (see `details_queue`). [`read_batch`]
//! asks `cancelled` before each read from the disk and gives up as soon as
//! it says so. A link reports what it points to; a broken link reports
//! itself. An entry that can't be read (it vanished, say) gets empty
//! details instead of stopping the rest.

use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use nova_files_api::{Details, EntryKind, Listing};

/// The details of each entry `entries` names by its path and kind, in
/// the same order. `None` when `cancelled` said so before the batch was
/// done.
pub fn read_batch(
    entries: impl ExactSizeIterator<Item = (PathBuf, EntryKind)>,
    cancelled: impl Fn() -> bool,
) -> Option<Vec<Details>> {
    let mut details = Vec::with_capacity(entries.len());
    for (path, kind) in entries {
        details.push(read_one(&path, kind, &cancelled)?);
    }
    Some(details)
}

/// `listing`'s entries `range` as [`read_batch`] takes them; entries past
/// its end are left out.
pub fn listed(
    listing: &Listing,
    range: Range<usize>,
) -> impl ExactSizeIterator<Item = (PathBuf, EntryKind)> + '_ {
    let entries = listing.entries.get(range).unwrap_or_default();
    entries
        .iter()
        .map(|entry| (listing.path_of(entry), entry.kind))
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::os::unix::fs::symlink;

    use super::*;

    #[test]
    fn a_cancel_after_the_first_read_skips_the_broken_link_fallback() {
        let folder =
            std::env::temp_dir().join(format!("nova-files-details-cancel-{}", std::process::id()));
        fs::create_dir_all(&folder).ok();
        let link = folder.join("broken");
        fs::remove_file(&link).ok();
        symlink(folder.join("gone"), &link).ok();
        let asked = Cell::new(0);
        let cancelled = || {
            asked.set(asked.get() + 1);
            asked.get() > 1
        };

        let details = read_one(&link, EntryKind::File, cancelled);

        fs::remove_dir_all(&folder).ok();
        assert_eq!(details, None);
        assert_eq!(asked.get(), 2);
    }
}
