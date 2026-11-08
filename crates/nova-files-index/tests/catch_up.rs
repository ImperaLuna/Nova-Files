//! Catching up on start: a home is crawled, changed while nothing
//! watches, then watched; the log must hold what changed.

#![expect(clippy::unwrap_used, reason = "test setup should fail loudly")]

mod support;

use std::fs;
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
    assert!(opened.caught.is_empty());
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
fn moves_while_closed_are_moves() {
    let closed = Closed::new(&["a/file.txt", "a/folder/inner.txt", "b/"]);
    fs::rename(closed.path("a/file.txt"), closed.path("b/file.txt")).unwrap();
    fs::rename(closed.path("a/folder"), closed.path("b/folder")).unwrap();
    let mut opened = closed.open();
    let mut moves: Vec<Change> = entries(std::mem::take(&mut opened.caught));
    moves.sort_by_key(|change| format!("{change:?}"));
    assert_eq!(
        moves,
        [
            Change::Moved {
                from: closed.path("a/file.txt"),
                to: closed.path("b/file.txt")
            },
            Change::Moved {
                from: closed.path("a/folder"),
                to: closed.path("b/folder")
            },
        ]
    );
    // The moved folder is watched at its new place.
    let later = closed.path("b/folder/later.txt");
    fs::write(&later, b"").unwrap();
    assert!(added_paths(&opened.next()).contains(&later.as_path()));
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
    assert!(closed.caught_up().is_empty());
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
fn changes_inside_a_folder_moved_while_closed_are_found() {
    let closed = Closed::new(&["a/folder/old.txt", "a/folder/inner/deep.txt", "b/"]);
    fs::rename(closed.path("a/folder"), closed.path("b/folder")).unwrap();
    fs::write(closed.path("b/folder/new.txt"), b"").unwrap();
    fs::write(closed.path("b/folder/inner/deeper.txt"), b"").unwrap();
    // Last, so neither new file gets its inode.
    fs::remove_file(closed.path("b/folder/old.txt")).unwrap();
    let changes = entries(closed.caught_up());
    assert!(changes.contains(&Change::Moved {
        from: closed.path("a/folder"),
        to: closed.path("b/folder")
    }));
    assert!(
        changes.contains(&Change::Removed {
            path: closed.path("b/folder/old.txt")
        }),
        "{changes:#?}"
    );
    let expected =
        ["b/folder/inner/deeper.txt", "b/folder/new.txt"].map(|relative| closed.path(relative));
    let expected: Vec<&Path> = expected.iter().map(PathBuf::as_path).collect();
    assert_eq!(added_paths(&changes), expected);
}
