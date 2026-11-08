//! Catching up on start: a home is crawled, changed while nothing
//! watches, then watched; the log must hold what changed.

#![expect(clippy::unwrap_used, reason = "test setup should fail loudly")]

mod support;

use std::fs;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use nova_files_index::{Change, ChangeLog, Index, LogReader, Watcher, crawl};
use support::TempFolder;

/// How long a test waits for a change the watcher should still report.
const WAIT: Duration = Duration::from_secs(5);

/// How long ago the folders' times are set when a test needs them settled.
const LONG_AGO: Duration = Duration::from_hours(1);

/// A home crawled from `paths`, waiting to be changed and caught up.
struct Closed {
    home: TempFolder,
    cache: TempFolder,
}

impl Closed {
    fn new(paths: &[&str]) -> Self {
        Self::crawled(TempFolder::new().with(paths))
    }

    /// Like `new`, with every folder's time an hour old, as if nothing
    /// had happened in them lately.
    fn settled(paths: &[&str]) -> Self {
        let home = TempFolder::new().with(paths);
        let old = SystemTime::now() - LONG_AGO;
        for path in paths {
            let mut folder = home.path().join(path);
            while folder.pop() && folder.starts_with(home.path()) {
                fs::File::open(&folder).unwrap().set_modified(old).unwrap();
            }
        }
        Self::crawled(home)
    }

    fn crawled(home: TempFolder) -> Self {
        let cache = TempFolder::new();
        crawl(home.path(), || false)
            .unwrap()
            .save(&cache.path().join("index"))
            .unwrap();
        Self { home, cache }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.home.path().join(relative)
    }

    /// Starts watching, with the catch-up already logged.
    fn open(&self) -> Opened {
        let index = Index::open(&self.cache.path().join("index")).unwrap();
        let log_path = self.cache.path().join("changes");
        let log = ChangeLog::create(&log_path, index.crawled()).unwrap();
        let mut reader = LogReader::open(&log_path).unwrap();
        let (tell, woken) = mpsc::channel();
        let watcher = Watcher::start(&index, log, move || {
            tell.send(()).ok();
        })
        .unwrap();
        // The catch-up is logged, and told, before start returns.
        let caught = reader.read_new().unwrap();
        let told = woken.try_iter().count();
        Opened {
            caught,
            told,
            _watcher: watcher,
            reader,
            woken,
        }
    }

    fn caught_up(&self) -> Vec<Change> {
        self.open().caught
    }
}

/// A watcher started on a closed home.
struct Opened {
    /// What the catch-up logged.
    caught: Vec<Change>,
    /// How many times it was told before start returned.
    told: usize,
    _watcher: Watcher,
    reader: LogReader,
    woken: mpsc::Receiver<()>,
}

impl Opened {
    /// What the watcher logs next.
    fn next(&mut self) -> Vec<Change> {
        self.woken.recv_timeout(WAIT).unwrap();
        self.reader.read_new().unwrap()
    }
}

/// The changes that aren't a folder's own details changing.
fn entries(changes: Vec<Change>) -> Vec<Change> {
    changes
        .into_iter()
        .filter(|change| !matches!(change, Change::Changed { details, .. } if details.kind == nova_files_index::Kind::Folder))
        .collect()
}

fn added_paths(changes: &[Change]) -> Vec<&Path> {
    let mut paths: Vec<&Path> = changes
        .iter()
        .filter_map(|change| match change {
            Change::Added { path, .. } => Some(path.as_path()),
            _ => None,
        })
        .collect();
    paths.sort();
    paths
}

#[test]
fn nothing_changed_logs_nothing() {
    let closed = Closed::new(&["docs/a.txt", "music/"]);
    let opened = closed.open();
    assert_eq!(opened.caught, []);
    assert_eq!(opened.told, 0);
}

#[test]
fn new_and_deleted_files_are_found() {
    let closed = Closed::new(&["docs/old.txt"]);
    fs::write(closed.path("docs/new.txt"), b"").unwrap();
    fs::remove_file(closed.path("docs/old.txt")).unwrap();
    let changes = entries(closed.caught_up());
    assert_eq!(changes.len(), 2, "{changes:?}");
    assert!(changes.contains(&Change::Removed {
        path: closed.path("docs/old.txt")
    }));
    assert_eq!(
        added_paths(&changes),
        [closed.path("docs/new.txt").as_path()]
    );
}

#[test]
fn a_rewritten_file_is_changed_where_its_folder_changed() {
    let closed = Closed::new(&["docs/a.txt", "docs/b.txt"]);
    fs::write(closed.path("docs/a.txt"), b"much longer than before").unwrap();
    // A new file moves the folder's time, so it is read again.
    fs::write(closed.path("docs/c.txt"), b"").unwrap();
    let changes = entries(closed.caught_up());
    let rewritten: Vec<&Path> = changes
        .iter()
        .filter_map(|change| match change {
            Change::Changed { path, .. } => Some(path.as_path()),
            _ => None,
        })
        .collect();
    assert_eq!(rewritten, [closed.path("docs/a.txt").as_path()]);
}

#[test]
fn a_new_folder_is_added_whole_and_watched() {
    let closed = Closed::new(&["docs/"]);
    fs::create_dir_all(closed.path("docs/new/inner")).unwrap();
    fs::write(closed.path("docs/new/inner/file.txt"), b"").unwrap();
    fs::create_dir_all(closed.path("docs/.hidden")).unwrap();
    let mut opened = closed.open();
    assert_eq!(opened.told, 1);
    let expected = ["docs/new", "docs/new/inner", "docs/new/inner/file.txt"]
        .map(|relative| closed.path(relative));
    let expected: Vec<&Path> = expected.iter().map(PathBuf::as_path).collect();
    assert_eq!(added_paths(&opened.caught), expected);
    let later = closed.path("docs/new/inner/later.txt");
    fs::write(&later, b"").unwrap();
    let after = opened.next();
    assert!(added_paths(&after).contains(&later.as_path()), "{after:?}");
}

#[test]
fn a_file_moved_while_closed_is_removed_then_added() {
    let closed = Closed::new(&["a/file.txt", "b/"]);
    let inode = fs::metadata(closed.path("a/file.txt")).unwrap().ino();
    fs::rename(closed.path("a/file.txt"), closed.path("b/file.txt")).unwrap();
    let changes = entries(closed.caught_up());
    let [
        Change::Removed { path: removed },
        Change::Added {
            path: added,
            details,
        },
    ] = changes.as_slice()
    else {
        panic!("{changes:?}");
    };
    assert_eq!(
        (removed, added),
        (&closed.path("a/file.txt"), &closed.path("b/file.txt"))
    );
    // The addition still tells it is the same file.
    assert_eq!(details.id.inode, inode);
}

#[test]
fn a_folder_moved_while_closed_asks_for_a_rescan() {
    let closed = Closed::new(&["a/folder/inner.txt", "a/file.txt", "b/"]);
    fs::rename(closed.path("a/folder"), closed.path("b/folder")).unwrap();
    fs::rename(closed.path("a/file.txt"), closed.path("b/file.txt")).unwrap();
    assert_eq!(
        closed.caught_up(),
        [Change::Rescan {
            folder: closed.path("")
        }]
    );
}

#[test]
fn a_name_holding_another_file_is_removed_then_added() {
    let closed = Closed::new(&["docs/a.txt"]);
    let path = closed.path("docs/a.txt");
    // Made while the old one still holds its inode, so it gets another;
    // the rename replaces the old file.
    fs::write(closed.path("a.tmp"), b"new").unwrap();
    fs::rename(closed.path("a.tmp"), &path).unwrap();
    let changes = entries(closed.caught_up());
    let removed = changes
        .iter()
        .position(|change| *change == Change::Removed { path: path.clone() });
    let added = changes
        .iter()
        .position(|change| matches!(change, Change::Added { path: at, .. } if *at == path));
    assert!(removed.unwrap() < added.unwrap(), "{changes:?}");
}

#[test]
fn a_folder_s_own_change_is_logged() {
    let closed = Closed::new(&["docs/"]);
    fs::write(closed.path("docs/new.txt"), b"").unwrap();
    let changes = closed.caught_up();
    assert!(changes.iter().any(|change| matches!(
        change,
        Change::Changed { path, details } if *path == closed.path("docs") && details.kind == nova_files_index::Kind::Folder
    )));
}

#[test]
fn folders_untouched_lately_are_not_read_again() {
    let closed = Closed::settled(&["docs/a.txt"]);
    // Rewriting a file leaves its folder's time alone.
    fs::write(closed.path("docs/a.txt"), b"longer now").unwrap();
    assert_eq!(closed.caught_up(), []);
}

#[test]
fn folders_touched_just_before_the_crawl_are_read_again() {
    let closed = Closed::new(&["docs/a.txt"]);
    fs::write(closed.path("docs/a.txt"), b"longer now").unwrap();
    let changes = closed.caught_up();
    assert!(
        changes.iter().any(|change| matches!(
            change,
            Change::Changed { path, .. } if *path == closed.path("docs/a.txt")
        )),
        "{changes:?}"
    );
}

#[test]
fn a_file_written_and_moved_while_closed_is_added_as_it_is_now() {
    let closed = Closed::new(&["a/file.txt", "b/"]);
    fs::write(closed.path("a/file.txt"), b"longer than before").unwrap();
    fs::rename(closed.path("a/file.txt"), closed.path("b/file.txt")).unwrap();
    let changes = entries(closed.caught_up());
    let to = closed.path("b/file.txt");
    assert!(
        changes.iter().any(
            |change| matches!(change, Change::Added { path, details } if *path == to && details.size == 18)
        ),
        "{changes:?}"
    );
}

#[test]
fn a_folder_that_became_a_profile_while_closed_is_removed() {
    let closed = Closed::new(&["apps/browser/bookmarks.html"]);
    fs::write(closed.path("apps/browser/profiles.ini"), b"").unwrap();
    let mut opened = closed.open();
    let changes = entries(std::mem::take(&mut opened.caught));
    assert_eq!(
        changes,
        [Change::Removed {
            path: closed.path("apps/browser")
        }]
    );
}

#[test]
fn nothing_is_read_through_a_link_that_replaced_a_folder() {
    let outside = TempFolder::new().with(&["b/new.txt"]);
    let closed = Closed::new(&["a/b/old.txt"]);
    fs::remove_dir_all(closed.path("a")).unwrap();
    std::os::unix::fs::symlink(outside.path(), closed.path("a")).unwrap();
    let changes = entries(closed.caught_up());
    assert!(
        changes.iter().all(|change| match change {
            Change::Added { path, .. } | Change::Changed { path, .. } =>
                !path.starts_with(closed.path("a/b")),
            _ => true,
        }),
        "{changes:?}"
    );
}

#[test]
fn a_folder_replaced_by_another_while_closed_asks_for_a_rescan() {
    let closed = Closed::new(&["a/one.txt", "b/two.txt"]);
    // `b` now holds what was `a`, and what was `b` is `c`: comparing the
    // new `b` with the old one's rows would lose `two.txt`.
    fs::rename(closed.path("b"), closed.path("c")).unwrap();
    fs::rename(closed.path("a"), closed.path("b")).unwrap();
    assert_eq!(
        closed.caught_up(),
        [Change::Rescan {
            folder: closed.path("")
        }]
    );
}

#[test]
fn swapped_names_are_logged_so_both_survive_a_replay() {
    let closed = Closed::new(&["a.txt", "b.txt"]);
    fs::rename(closed.path("a.txt"), closed.path("tmp")).unwrap();
    fs::rename(closed.path("b.txt"), closed.path("a.txt")).unwrap();
    fs::rename(closed.path("tmp"), closed.path("b.txt")).unwrap();
    let changes = entries(closed.caught_up());
    // Both leave before either comes back.
    let removed = changes
        .iter()
        .filter(|change| matches!(change, Change::Removed { .. }))
        .count();
    assert_eq!(removed, 2, "{changes:?}");
    assert!(matches!(
        &changes[2..],
        [Change::Added { .. }, Change::Added { .. }]
    ));
    let expected = [closed.path("a.txt"), closed.path("b.txt")];
    assert_eq!(
        added_paths(&changes),
        expected.each_ref().map(PathBuf::as_path)
    );
}
