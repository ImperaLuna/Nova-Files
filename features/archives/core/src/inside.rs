use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::env;
use std::ffi::OsString;
use std::fs::{self, DirBuilder};
use std::os::unix::fs::DirBuilderExt as _;
use std::path::{Component, Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nova_files_api::{Details, Entry, EntryId, EntryKind, HiddenFiles, Order, SortKey, Unreadable};
use nova_files_disk::private_file::FOLDER_MODE;
use nova_files_entries_core::{Inner, InnerFolders};
use nova_files_open_api::{NotOpened, TakeOut};

use crate::{ArchiveEntry, ArchiveFailure, ArchiveKind, Archiver, Flow};

const KEPT: usize = 4;
const OPENED: &str = "nova-files/opened";
const TRIES: u32 = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Within {
    pub archive: PathBuf,
    pub inner: PathBuf,
}

pub fn within(path: &Path) -> Option<Within> {
    if !path.is_absolute() {
        return None;
    }
    let (archive, metadata) = path
        .ancestors()
        .find_map(|part| fs::metadata(part).ok().map(|metadata| (part, metadata)))?;
    let inner = path.strip_prefix(archive).ok()?;
    let normal = inner
        .components()
        .all(|part| matches!(part, Component::Normal(_)));
    (metadata.is_file() && normal).then(|| Within {
        archive: archive.to_path_buf(),
        inner: inner.to_path_buf(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub kind: EntryKind,
    pub details: Details,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Tree {
    folders: HashMap<PathBuf, BTreeMap<OsString, Item>>,
}

impl Tree {
    pub fn of(entries: &[ArchiveEntry]) -> Self {
        let mut tree = Self::default();
        tree.folders.insert(PathBuf::new(), BTreeMap::new());
        for entry in entries {
            let kind = match entry.kind {
                ArchiveKind::Folder => EntryKind::Folder,
                ArchiveKind::File | ArchiveKind::Link => EntryKind::File,
            };
            let details = Details {
                size: (kind == EntryKind::File).then_some(entry.size),
                modified: time(entry.modified),
            };
            tree.add(&entry.path, Some(Item { kind, details }));
        }
        tree
    }

    fn add(&mut self, path: &Path, item: Option<Item>) {
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return;
        };
        if !self.folders.contains_key(parent) {
            self.add(parent, None);
        }
        let item = item.unwrap_or_else(|| Item {
            kind: EntryKind::Folder,
            details: Details::default(),
        });
        if item.kind == EntryKind::Folder {
            self.folders.entry(path.to_path_buf()).or_default();
        }
        let items = self.folders.entry(parent.to_path_buf()).or_default();
        items.insert(name.to_os_string(), item);
    }

    pub fn folder(&self, inner: &Path) -> Result<(Vec<Entry>, Vec<Details>), Unreadable> {
        let Some(items) = self.folders.get(inner) else {
            let file = inner
                .parent()
                .zip(inner.file_name())
                .and_then(|(parent, name)| self.folders.get(parent)?.get(name));
            return Err(file.map_or(Unreadable::Missing, |_| Unreadable::NotAFolder));
        };
        let mut sorted: Vec<_> = items.iter().collect();
        sorted.sort_by_key(|(_, item)| item.kind == EntryKind::File);
        Ok(sorted
            .into_iter()
            .zip(0..)
            .map(|((name, item), index)| {
                let entry = Entry {
                    id: EntryId(index),
                    name: name.clone(),
                    kind: item.kind,
                };
                (entry, item.details)
            })
            .unzip())
    }
}

pub fn arrange(
    (entries, details): (Vec<Entry>, Vec<Details>),
    order: Order,
    hidden: HiddenFiles,
) -> (Vec<Entry>, Vec<Details>) {
    let mut paired: Vec<(Entry, Details)> = entries
        .into_iter()
        .zip(details)
        .filter(|(entry, _)| hidden == HiddenFiles::Shown || !entry.is_hidden())
        .collect();
    paired.sort_by(|(left, left_details), (right, right_details)| {
        let by_key = match order.key {
            SortKey::Name => left.name.cmp(&right.name),
            SortKey::Modified => left_details.modified.cmp(&right_details.modified),
            SortKey::Size => left_details.size.cmp(&right_details.size),
            SortKey::Kind => Ordering::Equal,
        };
        let by_key = if order.descending {
            by_key.reverse()
        } else {
            by_key
        };
        (left.kind == EntryKind::File)
            .cmp(&(right.kind == EntryKind::File))
            .then(by_key)
    });
    paired
        .into_iter()
        .zip(0..)
        .map(|((mut entry, details), index)| {
            entry.id = EntryId(index);
            (entry, details)
        })
        .unzip()
}

pub fn opened_folder(runtime: Option<OsString>) -> PathBuf {
    runtime
        .map_or_else(env::temp_dir, PathBuf::from)
        .join(OPENED)
}

fn time(seconds: i64) -> Option<SystemTime> {
    let since = Duration::from_secs(seconds.unsigned_abs());
    if seconds.is_negative() {
        UNIX_EPOCH.checked_sub(since)
    } else {
        UNIX_EPOCH.checked_add(since)
    }
}

type Stamp = (u64, Option<SystemTime>);

#[derive(Default)]
struct Kept(VecDeque<(PathBuf, Stamp, Arc<Tree>)>);

impl Kept {
    fn find(&self, archive: &Path, stamp: Stamp) -> Option<Arc<Tree>> {
        self.0
            .iter()
            .find(|(path, kept, _)| path == archive && *kept == stamp)
            .map(|(_, _, tree)| Arc::clone(tree))
    }

    fn keep(&mut self, archive: &Path, stamp: Stamp, tree: Arc<Tree>) {
        self.0.retain(|(path, _, _)| path != archive);
        self.0.push_front((archive.to_path_buf(), stamp, tree));
        self.0.truncate(KEPT);
    }
}

pub struct Inside {
    archiver: Option<Archiver>,
    kept: Mutex<Kept>,
    opened: PathBuf,
    taken: AtomicU64,
}

impl Inside {
    pub fn new(archiver: Option<Archiver>, opened: PathBuf) -> Self {
        Self {
            archiver,
            kept: Mutex::default(),
            opened,
            taken: AtomicU64::new(0),
        }
    }

    fn copy_out(&self, within: &Within) -> Result<PathBuf, ArchiveFailure> {
        let archiver = self.archiver.as_ref().ok_or(ArchiveFailure::NotRead)?;
        let folder = self.new_folder().ok_or(ArchiveFailure::NotWritten)?;
        let picked = std::slice::from_ref(&within.inner);
        archiver.extract_picked((&within.archive, picked), &folder, None, |_| Flow::Go)?;
        let copy = folder.join(&within.inner);
        if copy.symlink_metadata().is_err() {
            return Err(ArchiveFailure::Gone);
        }
        Ok(copy)
    }

    fn new_folder(&self) -> Option<PathBuf> {
        DirBuilder::new()
            .recursive(true)
            .mode(FOLDER_MODE)
            .create(&self.opened)
            .ok()?;
        (0..TRIES)
            .map(|_| {
                let taken = self.taken.fetch_add(1, AtomicOrdering::Relaxed);
                self.opened.join(format!("{}-{taken}", process::id()))
            })
            .find(|folder| DirBuilder::new().mode(FOLDER_MODE).create(folder).is_ok())
    }

    pub fn tree(&self, archive: &Path) -> Result<Arc<Tree>, Unreadable> {
        let metadata = fs::metadata(archive)
            .map_err(|error| nova_files_entries_core::list::unreadable(&error))?;
        let stamp = (metadata.len(), metadata.modified().ok());
        let kept = self.lock().find(archive, stamp);
        if let Some(tree) = kept {
            return Ok(tree);
        }
        let archiver = self.archiver.as_ref().ok_or(Unreadable::NotAFolder)?;
        let entries = archiver.list(archive).map_err(unreadable)?;
        let tree = Arc::new(Tree::of(&entries));
        self.lock().keep(archive, stamp, Arc::clone(&tree));
        Ok(tree)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Kept> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl InnerFolders for Inside {
    fn list(&self, path: &Path, order: Order, hidden: HiddenFiles) -> Option<Inner> {
        let within = within(path)?;
        let listed = self
            .tree(&within.archive)
            .and_then(|tree| tree.folder(&within.inner));
        Some(listed.map(|listed| arrange(listed, order, hidden)))
    }
}

impl TakeOut for Inside {
    fn take_out(&self, path: &Path) -> Option<Result<PathBuf, NotOpened>> {
        within(path).map(|within| self.copy_out(&within).map_err(|_| NotOpened::Failed))
    }
}

const fn unreadable(failure: ArchiveFailure) -> Unreadable {
    match failure {
        ArchiveFailure::Gone => Unreadable::Missing,
        ArchiveFailure::Denied | ArchiveFailure::NeedsPassword => Unreadable::PermissionDenied,
        ArchiveFailure::Unsupported => Unreadable::NotAFolder,
        _ => Unreadable::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_files_test_folder::TestFolder;

    fn entry(path: &str, kind: ArchiveKind, size: u64) -> ArchiveEntry {
        ArchiveEntry {
            kind,
            locked: false,
            size,
            modified: 60,
            mode: 0o644,
            path: path.into(),
            link: PathBuf::new(),
        }
    }

    fn names(listed: &(Vec<Entry>, Vec<Details>)) -> Vec<(String, EntryKind)> {
        listed
            .0
            .iter()
            .map(|entry| (entry.name.to_string_lossy().into_owned(), entry.kind))
            .collect()
    }

    #[test]
    fn folders_come_from_their_entries_and_paths() {
        let tree = Tree::of(&[
            entry("b.txt", ArchiveKind::File, 2),
            entry("docs/inner/deep.txt", ArchiveKind::File, 4),
            entry("docs", ArchiveKind::Folder, 0),
            entry("a-link", ArchiveKind::Link, 5),
        ]);
        let top = tree.folder(Path::new("")).unwrap();
        assert_eq!(
            names(&top),
            [
                ("docs".to_owned(), EntryKind::Folder),
                ("a-link".to_owned(), EntryKind::File),
                ("b.txt".to_owned(), EntryKind::File),
            ]
        );
        assert_eq!(
            top.0.iter().map(|entry| entry.id.0).collect::<Vec<_>>(),
            [0, 1, 2]
        );
        let modified = time(60);
        assert_eq!(
            top.1,
            [
                Details {
                    size: None,
                    modified
                },
                Details {
                    size: Some(5),
                    modified
                },
                Details {
                    size: Some(2),
                    modified
                },
            ]
        );
        let inner = tree.folder(Path::new("docs")).unwrap();
        assert_eq!(names(&inner), [("inner".to_owned(), EntryKind::Folder)]);
        assert_eq!(inner.1, [Details::default()]);
        let deep = tree.folder(Path::new("docs/inner")).unwrap();
        assert_eq!(
            deep.1,
            [Details {
                size: Some(4),
                modified
            }]
        );
        assert_eq!(tree.folder(Path::new("b.txt")), Err(Unreadable::NotAFolder));
        assert_eq!(tree.folder(Path::new("nothing")), Err(Unreadable::Missing));
        assert_eq!(
            tree.folder(Path::new("docs/nothing")),
            Err(Unreadable::Missing)
        );
    }

    #[test]
    fn keeps_the_newest_archives_while_unchanged() {
        let mut kept = Kept::default();
        let stamp = (1, Some(UNIX_EPOCH));
        let tree = |path: &str| Arc::new(Tree::of(&[entry(path, ArchiveKind::File, 1)]));
        for name in ["a", "b", "c", "d", "e"] {
            kept.keep(Path::new(name), stamp, tree(name));
        }
        assert_eq!(kept.find(Path::new("a"), stamp), None);
        assert_eq!(kept.find(Path::new("b"), stamp), Some(tree("b")));
        assert_eq!(kept.find(Path::new("e"), (2, Some(UNIX_EPOCH))), None);
        assert_eq!(kept.find(Path::new("e"), (1, None)), None);
        kept.keep(Path::new("e"), (2, None), tree("new"));
        assert_eq!(kept.find(Path::new("e"), stamp), None);
        assert_eq!(kept.find(Path::new("e"), (2, None)), Some(tree("new")));
        assert_eq!(kept.0.len(), KEPT);
    }

    #[test]
    fn an_entry_of_its_own_replaces_a_folder_a_path_named() {
        let tree = Tree::of(&[
            entry("docs/a.txt", ArchiveKind::File, 1),
            entry("docs", ArchiveKind::Folder, 0),
            entry("docs/b.txt", ArchiveKind::File, 1),
        ]);
        let top = tree.folder(Path::new("")).unwrap();
        assert_eq!(
            top.1,
            [Details {
                size: None,
                modified: time(60)
            }]
        );
        assert_eq!(tree.folder(Path::new("docs")).unwrap().0.len(), 2);
    }

    #[test]
    fn an_empty_archive_has_an_empty_top() {
        let tree = Tree::of(&[]);
        assert_eq!(tree.folder(Path::new("")), Ok((Vec::new(), Vec::new())));
    }

    #[test]
    fn times_before_1970_count() {
        assert_eq!(time(-60), UNIX_EPOCH.checked_sub(Duration::from_secs(60)));
        assert_eq!(time(0), Some(UNIX_EPOCH));
        assert_eq!(time(60), UNIX_EPOCH.checked_add(Duration::from_secs(60)));
    }

    #[test]
    fn a_path_is_inside_where_a_file_ends_it() {
        let folder = TestFolder::new("within");
        let root = folder.path();
        fs::create_dir_all(root.join("folder")).unwrap();
        fs::write(root.join("a.zip"), b"").unwrap();
        assert_eq!(
            within(&root.join("a.zip/docs/x.txt")),
            Some(Within {
                archive: root.join("a.zip"),
                inner: "docs/x.txt".into(),
            })
        );
        assert_eq!(
            within(&root.join("a.zip")),
            Some(Within {
                archive: root.join("a.zip"),
                inner: PathBuf::new(),
            })
        );
        assert_eq!(within(&root.join("folder/gone")), None);
        assert_eq!(within(&root.join("folder")), None);
        assert_eq!(within(&root.join("a.zip/../folder")), None);
        assert_eq!(within(Path::new("a.zip/docs")), None);
    }

    #[test]
    fn failures_say_why_the_contents_are_unreadable() {
        assert_eq!(unreadable(ArchiveFailure::Gone), Unreadable::Missing);
        assert_eq!(
            unreadable(ArchiveFailure::Denied),
            Unreadable::PermissionDenied
        );
        assert_eq!(
            unreadable(ArchiveFailure::NeedsPassword),
            Unreadable::PermissionDenied
        );
        assert_eq!(
            unreadable(ArchiveFailure::Unsupported),
            Unreadable::NotAFolder
        );
        assert_eq!(unreadable(ArchiveFailure::Broken), Unreadable::Failed);
    }

    #[test]
    fn hidden_entries_show_only_when_asked() {
        let tree = Tree::of(&[
            entry(".hidden", ArchiveKind::File, 1),
            entry("shown", ArchiveKind::File, 1),
        ]);
        let listed = || tree.folder(Path::new("")).unwrap();
        let order = Order::default();
        let left = arrange(listed(), order, HiddenFiles::Left);
        assert_eq!(names(&left), [("shown".to_owned(), EntryKind::File)]);
        assert_eq!(left.0[0].id, EntryId(0));
        let shown = arrange(listed(), order, HiddenFiles::Shown);
        assert_eq!(shown.0.len(), 2);
    }

    #[test]
    fn opened_files_go_under_the_runtime_folder() {
        assert_eq!(
            opened_folder(Some("/run/user/7".into())),
            Path::new("/run/user/7/nova-files/opened")
        );
        let fallback = opened_folder(None);
        assert_eq!(
            fallback.strip_prefix(env::temp_dir()),
            Ok(Path::new(OPENED))
        );
    }

    #[test]
    fn each_file_opened_gets_a_new_folder() {
        let folder = TestFolder::new("opened");
        let root = folder.join("root");
        let inside = Inside::new(None, root.join("opened"));
        fs::create_dir_all(root.join(format!("opened/{}-0", process::id()))).unwrap();
        assert_eq!(
            inside.new_folder(),
            Some(root.join(format!("opened/{}-1", process::id())))
        );
        fs::write(root.join("file"), b"").unwrap();
        assert_eq!(
            Inside::new(None, root.join("file/opened")).new_folder(),
            None
        );
    }
}
