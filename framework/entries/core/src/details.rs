use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use nova_files_api::{Details, Entry, EntryKind, Listing};

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

pub fn listed(
    listing: &Listing,
    range: Range<usize>,
) -> impl ExactSizeIterator<Item = (PathBuf, EntryKind)> + '_ {
    entries(listing, range)
        .iter()
        .map(|entry| (listing.path_of(entry), entry.kind))
}

pub fn entries(listing: &Listing, range: Range<usize>) -> &[Entry] {
    listing.entries.get(range).unwrap_or_default()
}

fn read_one(path: &Path, kind: EntryKind, cancelled: impl Fn() -> bool) -> Option<Details> {
    if cancelled() {
        return None;
    }
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
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
