use std::fs;
use std::path::PathBuf;

use nova_files_api::{Entry, EntryId, EntryKind, Listing, ListingId};
use nova_files_search_api::Hit;

pub fn listing(
    id: ListingId,
    folder: PathBuf,
    hits: Vec<Hit>,
    cancelled: impl Fn() -> bool,
) -> Option<Listing> {
    let mut entries = Vec::with_capacity(hits.len());
    let mut folders = Vec::with_capacity(hits.len());
    for hit in hits {
        if cancelled() {
            return None;
        }
        let (Some(name), Some(parent)) = (hit.path.file_name(), hit.path.parent()) else {
            continue;
        };
        let kind = match fs::metadata(&hit.path) {
            Ok(metadata) if metadata.is_dir() => EntryKind::Folder,
            Ok(_) => EntryKind::File,
            Err(_) if fs::symlink_metadata(&hit.path).is_ok() => EntryKind::File,
            Err(_) => continue,
        };
        let Ok(index) = u32::try_from(entries.len()) else {
            break;
        };
        entries.push(Entry {
            id: EntryId(index),
            name: name.to_owned(),
            kind,
        });
        folders.push(parent.to_owned());
    }
    Some(Listing {
        id,
        path: folder,
        entries: entries.into(),
        folders: Some(folders.into()),
    })
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use nova_files_search_api::Matched;

    use nova_files_test_folder::TestFolder;

    use super::*;

    fn hit(path: PathBuf) -> Hit {
        Hit {
            path,
            matched: Matched::Contains,
        }
    }

    #[test]
    fn hits_become_entries_in_their_own_folders_and_gone_ones_are_left_out() {
        let folder = TestFolder::new("found");
        let root = folder.path().to_path_buf();
        fs::create_dir_all(root.join("docs/notes")).unwrap();
        fs::write(root.join("docs/notes.txt"), b"").unwrap();
        symlink(root.join("docs/notes"), root.join("notes-link")).unwrap();
        symlink(root.join("nowhere"), root.join("notes-broken")).unwrap();
        let hits = [
            "docs/notes",
            "gone/notes",
            "docs/notes.txt",
            "notes-link",
            "notes-broken",
        ]
        .map(|path| hit(root.join(path)));

        let listing = listing(ListingId(7), root.clone(), hits.to_vec(), || false).unwrap();

        let shown: Vec<_> = listing
            .entries
            .iter()
            .map(|entry| (listing.path_of(entry), entry.kind, entry.id))
            .collect();
        assert_eq!(
            shown,
            [
                (root.join("docs/notes"), EntryKind::Folder, EntryId(0)),
                (root.join("docs/notes.txt"), EntryKind::File, EntryId(1)),
                (root.join("notes-link"), EntryKind::Folder, EntryId(2)),
                (root.join("notes-broken"), EntryKind::File, EntryId(3)),
            ]
        );
        assert_eq!((listing.id, listing.path), (ListingId(7), root));
    }

    #[test]
    fn a_cancelled_listing_looks_at_nothing() {
        let hits = vec![hit(PathBuf::from("/"))];
        assert_eq!(
            listing(ListingId(0), PathBuf::from("/"), hits, || true),
            None
        );
    }
}
