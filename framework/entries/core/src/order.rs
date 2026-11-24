use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use nova_files_api::{Details, Entry, EntryId, EntryKind, Order, SortKey};
use nova_files_disk::ports::Icons;

use crate::details;
use crate::folder::Inner;

pub fn arrange(
    folder: &Path,
    entries: Vec<Entry>,
    order: Order,
    kinds: &dyn Icons,
    cancelled: impl Fn() -> bool,
) -> Option<Vec<Entry>> {
    let paired: Vec<(Entry, ())> = entries.into_iter().map(|entry| (entry, ())).collect();
    if order.key == SortKey::Kind {
        let sorted = by_kind(paired, kinds, order.descending, cancelled)?;
        return Some(sorted.into_iter().map(|(entry, ())| entry).collect());
    }
    let arranged = arrange_paired(
        paired,
        |entry, ()| folder.join(&entry.name),
        order,
        cancelled,
    )?;
    Some(arranged.into_iter().map(|(entry, ())| entry).collect())
}

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
        SortKey::Name | SortKey::Kind => entries,
        SortKey::Modified | SortKey::Size => {
            let sorted = sorted_with_details(
                entries,
                path,
                |_, _, _| {},
                |(left, left_details), (right, right_details)| {
                    group(left)
                        .cmp(&group(right))
                        .then_with(|| by_key(order, left_details, right_details))
                },
                cancelled,
            )?;
            return Some(sorted.into_iter().map(|(entry, _)| entry).collect());
        }
    };
    for ((entry, _), index) in entries.iter_mut().zip(0..) {
        entry.id = EntryId(index);
    }
    Some(entries)
}

pub(crate) fn sorted_with_details<T>(
    entries: Vec<(Entry, T)>,
    path: impl Fn(&Entry, &T) -> PathBuf,
    fill: impl Fn(&Entry, &T, &mut Details),
    compare: impl Fn((&Entry, &Details), (&Entry, &Details)) -> Ordering,
    cancelled: impl Fn() -> bool,
) -> Option<Vec<((Entry, T), Details)>> {
    let paths = entries
        .iter()
        .map(|(entry, carried)| (path(entry, carried), entry.kind));
    let read = details::read_batch(paths, cancelled)?;
    let mut paired: Vec<((Entry, T), Details)> = entries
        .into_iter()
        .zip(read)
        .map(|((entry, carried), mut details)| {
            fill(&entry, &carried, &mut details);
            ((entry, carried), details)
        })
        .collect();
    paired.sort_by(|((left, _), left_details), ((right, _), right_details)| {
        compare((left, left_details), (right, right_details))
    });
    for (((entry, _), _), index) in paired.iter_mut().zip(0..) {
        entry.id = EntryId(index);
    }
    Some(paired)
}

pub fn arrange_known(
    listed: Inner,
    order: Order,
    kinds: &dyn Icons,
    cancelled: impl Fn() -> bool,
) -> Option<Inner> {
    if order.key != SortKey::Kind {
        return Some(listed);
    }
    let Ok((entries, details)) = listed else {
        return Some(listed);
    };
    let paired = entries.into_iter().zip(details).collect();
    let sorted = by_kind(paired, kinds, order.descending, cancelled)?;
    Some(Ok(sorted.into_iter().unzip()))
}

fn by_kind<T>(
    entries: Vec<(Entry, T)>,
    kinds: &dyn Icons,
    descending: bool,
    cancelled: impl Fn() -> bool,
) -> Option<Vec<(Entry, T)>> {
    let mut named = Vec::with_capacity(entries.len());
    for (entry, carried) in entries {
        if cancelled() {
            return None;
        }
        let kind = kinds
            .kind_name(&entry.name, entry.kind)
            .map(|kind| kind.to_lowercase());
        named.push((kind, (entry, carried)));
    }
    named.sort_by(|(left_kind, (left, _)), (right_kind, (right, _))| {
        group(left)
            .cmp(&group(right))
            .then_with(|| known_first(left_kind.as_deref(), right_kind.as_deref(), descending))
    });
    let renumbered = named
        .into_iter()
        .zip(0..)
        .map(|((_, (entry, carried)), index)| {
            let entry = Entry {
                id: EntryId(index),
                ..entry
            };
            (entry, carried)
        })
        .collect();
    Some(renumbered)
}

fn group(entry: &Entry) -> bool {
    entry.kind == EntryKind::File
}

fn by_key(order: Order, left: &Details, right: &Details) -> Ordering {
    match order.key {
        SortKey::Modified => known_first(left.modified, right.modified, order.descending),
        SortKey::Size => known_first(left.size, right.size, order.descending),
        SortKey::Name | SortKey::Kind => Ordering::Equal,
    }
}

pub(crate) fn known_first<T: Ord>(left: Option<T>, right: Option<T>, descending: bool) -> Ordering {
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
    use std::ffi::OsStr;
    use std::fs::{self, File};
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    use nova_files_api::{FileIcon, IconKind};
    use nova_files_test_folder::TestFolder;

    use super::*;
    use crate::list;

    struct ByEnding;

    impl Icons for ByEnding {
        fn of(&self, _: &OsStr, _: EntryKind) -> FileIcon {
            FileIcon::BuiltIn(IconKind::File)
        }

        fn kind_name(&self, name: &OsStr, kind: EntryKind) -> Option<Arc<str>> {
            let ending = Path::new(name).extension()?.to_str()?;
            let word = match (kind, ending) {
                (EntryKind::File, "txt") => "Plain text",
                (EntryKind::File, "md") => "Markdown document",
                (EntryKind::File, "gltf") => "glTF model",
                _ => return None,
            };
            Some(Arc::from(word))
        }

        fn follow_desktop(&self, _: &[String]) {}
    }

    struct TempFolder(TestFolder);

    impl TempFolder {
        fn new(name: &str) -> Self {
            Self(TestFolder::new(&format!("order-{name}")))
        }

        fn file(&self, name: &str, size: u64, age: u64) {
            let file = File::create(self.0.join(name)).unwrap();
            file.set_len(size).unwrap();
            file.set_modified(SystemTime::now() - Duration::from_secs(age))
                .unwrap();
        }

        fn folder(&self, name: &str) {
            fs::create_dir(self.0.join(name)).unwrap();
        }

        fn names(&self, key: SortKey, descending: bool) -> Vec<String> {
            let entries = arranged(self.0.path(), Order { key, descending });
            entries
                .iter()
                .map(|entry| entry.name.to_string_lossy().into_owned())
                .collect()
        }
    }

    fn arranged(folder: &Path, order: Order) -> Vec<Entry> {
        let listed = list::read_folder(folder, || false).unwrap();
        arrange(folder, listed, order, &ByEnding, || false).unwrap()
    }

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
    fn by_type_files_group_by_kind_name_and_unnamed_kinds_go_last() {
        let folder = TempFolder::new("kind");
        folder.folder("b.txt");
        folder.folder("a");
        for name in [
            "z.txt",
            "notes.md",
            "big.bin",
            "m.gltf",
            "a.txt",
            "readme.md",
        ] {
            folder.file(name, 0, 0);
        }
        assert_eq!(
            folder.names(SortKey::Kind, false),
            [
                "a",
                "b.txt",
                "m.gltf",
                "notes.md",
                "readme.md",
                "a.txt",
                "z.txt",
                "big.bin"
            ]
        );
        assert_eq!(
            folder.names(SortKey::Kind, true),
            [
                "a",
                "b.txt",
                "a.txt",
                "z.txt",
                "notes.md",
                "readme.md",
                "m.gltf",
                "big.bin"
            ]
        );
    }

    #[test]
    fn an_inner_listing_by_type_keeps_each_entry_with_its_details() {
        let file = |index, name: &str| Entry {
            id: EntryId(index),
            name: name.into(),
            kind: EntryKind::File,
        };
        let details = |size| Details {
            size: Some(size),
            modified: None,
        };
        let listed = Ok((
            vec![file(0, "a.txt"), file(1, "b.md")],
            vec![details(1), details(2)],
        ));
        let order = Order {
            key: SortKey::Kind,
            descending: false,
        };
        assert_eq!(
            arrange_known(listed.clone(), order, &ByEnding, || false),
            Some(Ok((
                vec![file(0, "b.md"), file(1, "a.txt")],
                vec![details(2), details(1)]
            )))
        );
        assert_eq!(arrange_known(listed, order, &ByEnding, || true), None);
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
            folder.0.path(),
            Order {
                key: SortKey::Size,
                descending: true,
            },
        );
        for (entry, index) in entries.iter().zip(0..) {
            assert_eq!(entry.id, EntryId(index));
        }
        let entries = arranged(
            folder.0.path(),
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
        let listed = list::read_folder(folder.0.path(), || false).unwrap();
        fs::remove_file(folder.0.join("small.txt")).unwrap();
        let last = |descending| {
            let order = Order {
                key: SortKey::Size,
                descending,
            };
            let entries =
                arrange(folder.0.path(), listed.clone(), order, &ByEnding, || false).unwrap();
            entries.last().unwrap().name.to_string_lossy().into_owned()
        };
        assert_eq!(last(false), "small.txt");
        assert_eq!(last(true), "small.txt");
    }

    #[test]
    fn a_cancel_stops_before_the_next_read() {
        let folder = mixed("cancel");
        let listed = list::read_folder(folder.0.path(), || false).unwrap();
        let reads = Cell::new(0);
        let cancelled = || {
            reads.set(reads.get() + 1);
            reads.get() > 2
        };
        let order = Order {
            key: SortKey::Modified,
            descending: false,
        };
        assert_eq!(
            arrange(folder.0.path(), listed.clone(), order, &ByEnding, cancelled),
            None
        );
        let by_kind = Order {
            key: SortKey::Kind,
            descending: false,
        };
        assert_eq!(
            arrange(folder.0.path(), listed.clone(), by_kind, &ByEnding, || true),
            None
        );
        let by_name = Order::default();
        assert_eq!(
            arrange(folder.0.path(), listed.clone(), by_name, &ByEnding, || true),
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
