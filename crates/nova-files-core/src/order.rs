//! Putting a folder's entries in the order a client asked for.
//!
//! `list` reads a folder folders first, each group A to Z. [`arrange`]
//! takes that and orders each group by the client's key: by name it only
//! turns a group around; by modified time or size it reads every entry's
//! details first (see `details`), asking `cancelled` before each read.
//! The sort is stable, so entries the key can't tell apart (folders by
//! size, two files of one size) stay A to Z either way. An entry whose
//! details can't be read (it vanished) goes last, either way. Each entry's
//! id is then its new position.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use nova_files_api::{Details, Entry, EntryId, EntryKind, Order, SortKey};

use crate::details;

/// `entries`, listed folders first and each group A to Z, in `order`.
/// `None` when `cancelled` said so before every detail was read.
pub fn arrange(
    folder: &Path,
    entries: Vec<Entry>,
    order: Order,
    cancelled: impl Fn() -> bool,
) -> Option<Vec<Entry>> {
    let paired = entries.into_iter().map(|entry| (entry, ())).collect();
    let arranged = arrange_paired(
        paired,
        |entry, ()| folder.join(&entry.name),
        order,
        cancelled,
    )?;
    Some(arranged.into_iter().map(|(entry, ())| entry).collect())
}

/// [`arrange`] for entries each carrying a `T` that moves with it (such
/// as the folder it is in, for entries from several folders); `path` says
/// where each entry is, for its details.
pub fn arrange_paired<T>(
    entries: Vec<(Entry, T)>,
    path: impl Fn(&Entry, &T) -> PathBuf,
    order: Order,
    cancelled: impl Fn() -> bool,
) -> Option<Vec<(Entry, T)>> {
    let mut entries = match order.key {
        SortKey::Name if order.descending => {
            let mut entries = entries;
            let folders = entries.partition_point(|(entry, _)| entry.kind == EntryKind::Folder);
            let (folders, files) = entries.split_at_mut(folders);
            folders.reverse();
            files.reverse();
            entries
        }
        SortKey::Name => entries,
        SortKey::Modified | SortKey::Size => {
            let paths = entries
                .iter()
                .map(|(entry, carried)| (path(entry, carried), entry.kind));
            let read = details::read_batch(paths, cancelled)?;
            let mut paired: Vec<((Entry, T), Details)> = entries.into_iter().zip(read).collect();
            paired.sort_by(|((left, _), left_details), ((right, _), right_details)| {
                group(left)
                    .cmp(&group(right))
                    .then_with(|| by_key(order, left_details, right_details))
            });
            paired.into_iter().map(|(entry, _)| entry).collect()
        }
    };
    for ((entry, _), index) in entries.iter_mut().zip(0..) {
        entry.id = EntryId(index);
    }
    Some(entries)
}

/// Folders before files.
fn group(entry: &Entry) -> bool {
    entry.kind == EntryKind::File
}

/// How two entries' details compare under `order`'s key.
fn by_key(order: Order, left: &Details, right: &Details) -> Ordering {
    match order.key {
        SortKey::Modified => known_first(left.modified, right.modified, order.descending),
        SortKey::Size => known_first(left.size, right.size, order.descending),
        SortKey::Name => Ordering::Equal,
    }
}

/// Known values in the order asked, unknown ones after them.
fn known_first<T: Ord>(left: Option<T>, right: Option<T>, descending: bool) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) if descending => right.cmp(&left),
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::fs::{self, File};
    use std::time::{Duration, SystemTime};

    use super::*;
    use crate::list;

    /// A temp folder, removed when dropped.
    struct TempFolder(PathBuf);

    impl TempFolder {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("nova-files-order-{name}-{}", std::process::id()));
            fs::remove_dir_all(&path).ok();
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        /// A file of `size` bytes, last changed `age` seconds ago.
        fn file(&self, name: &str, size: u64, age: u64) {
            let file = File::create(self.0.join(name)).unwrap();
            file.set_len(size).unwrap();
            file.set_modified(SystemTime::now() - Duration::from_secs(age))
                .unwrap();
        }

        fn folder(&self, name: &str) {
            fs::create_dir(self.0.join(name)).unwrap();
        }

        /// The folder's entries in `order`, by name.
        fn names(&self, key: SortKey, descending: bool) -> Vec<String> {
            let entries = arranged(&self.0, Order { key, descending });
            entries
                .iter()
                .map(|entry| entry.name.to_string_lossy().into_owned())
                .collect()
        }
    }

    impl Drop for TempFolder {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    fn arranged(folder: &Path, order: Order) -> Vec<Entry> {
        let listed = list::read_folder(folder, || false).unwrap();
        arrange(folder, listed, order, || false).unwrap()
    }

    /// Folders `b` and `a`, then files: `big` (old), `small` (new) and
    /// `middle`, each a different size and age.
    fn mixed(name: &str) -> TempFolder {
        let folder = TempFolder::new(name);
        folder.folder("b");
        folder.folder("a");
        folder.file("big.bin", 3000, 300);
        folder.file("small.txt", 10, 10);
        folder.file("middle.md", 200, 100);
        folder
    }

    #[test]
    fn by_name_folders_come_first_and_each_group_turns_around() {
        let folder = mixed("name");
        assert_eq!(
            folder.names(SortKey::Name, false),
            ["a", "b", "big.bin", "middle.md", "small.txt"]
        );
        assert_eq!(
            folder.names(SortKey::Name, true),
            ["b", "a", "small.txt", "middle.md", "big.bin"]
        );
    }

    #[test]
    fn by_size_folders_stay_a_to_z_and_files_follow_their_size() {
        let folder = mixed("size");
        assert_eq!(
            folder.names(SortKey::Size, false),
            ["a", "b", "small.txt", "middle.md", "big.bin"]
        );
        assert_eq!(
            folder.names(SortKey::Size, true),
            ["a", "b", "big.bin", "middle.md", "small.txt"]
        );
    }

    #[test]
    fn by_modified_time_files_go_oldest_or_newest_first() {
        let folder = mixed("modified");
        let files = |descending| folder.names(SortKey::Modified, descending)[2..].to_vec();
        assert_eq!(files(false), ["big.bin", "middle.md", "small.txt"]);
        assert_eq!(files(true), ["small.txt", "middle.md", "big.bin"]);
    }

    #[test]
    fn equal_sizes_stay_a_to_z_either_way() {
        let folder = TempFolder::new("ties");
        folder.file("c", 5, 0);
        folder.file("a", 5, 0);
        folder.file("b", 5, 0);
        assert_eq!(folder.names(SortKey::Size, false), ["a", "b", "c"]);
        assert_eq!(folder.names(SortKey::Size, true), ["a", "b", "c"]);
    }

    #[test]
    fn ids_follow_the_new_positions() {
        let folder = mixed("ids");
        let entries = arranged(
            &folder.0,
            Order {
                key: SortKey::Size,
                descending: true,
            },
        );
        for (entry, index) in entries.iter().zip(0..) {
            assert_eq!(entry.id, EntryId(index));
        }
        let entries = arranged(
            &folder.0,
            Order {
                key: SortKey::Name,
                descending: true,
            },
        );
        for (entry, index) in entries.iter().zip(0..) {
            assert_eq!(entry.id, EntryId(index));
        }
    }

    #[test]
    fn an_entry_whose_details_cant_be_read_goes_last_either_way() {
        let folder = mixed("vanished");
        let listed = list::read_folder(&folder.0, || false).unwrap();
        fs::remove_file(folder.0.join("small.txt")).unwrap();
        let last = |descending| {
            let order = Order {
                key: SortKey::Size,
                descending,
            };
            let entries = arrange(&folder.0, listed.clone(), order, || false).unwrap();
            entries.last().unwrap().name.to_string_lossy().into_owned()
        };
        assert_eq!(last(false), "small.txt");
        assert_eq!(last(true), "small.txt");
    }

    #[test]
    fn a_cancel_stops_before_the_next_read() {
        let folder = mixed("cancel");
        let listed = list::read_folder(&folder.0, || false).unwrap();
        let reads = Cell::new(0);
        let cancelled = || {
            reads.set(reads.get() + 1);
            reads.get() > 2
        };
        let order = Order {
            key: SortKey::Modified,
            descending: false,
        };
        assert_eq!(arrange(&folder.0, listed.clone(), order, cancelled), None);
        // By name nothing is read, so nothing is asked.
        let by_name = Order::default();
        assert_eq!(
            arrange(&folder.0, listed.clone(), by_name, || true),
            Some(listed)
        );
    }

    #[test]
    fn unknown_values_come_after_known_ones() {
        assert_eq!(known_first(Some(1), None, false), Ordering::Less);
        assert_eq!(known_first(Some(1), None, true), Ordering::Less);
        assert_eq!(known_first(None, Some(1), true), Ordering::Greater);
        assert_eq!(known_first::<u8>(None, None, false), Ordering::Equal);
        assert_eq!(known_first(Some(1), Some(2), false), Ordering::Less);
        assert_eq!(known_first(Some(1), Some(2), true), Ordering::Greater);
    }
}
