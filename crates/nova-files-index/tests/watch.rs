//! The watcher against real temp folders: what each kind of change writes
//! to the change log, and what the rules keep out of it.

#![expect(clippy::unwrap_used, reason = "test setup should fail loudly")]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

mod support;

use support::TempFolder;

use nova_files_index::{Change, ChangeLog, Index, Kind, LogReader, Watcher, crawl};

/// How long a test waits for a change before it counts as a failure.
const WAIT: Duration = Duration::from_secs(5);
/// How long a test waits to be sure no change comes.
const QUIET: Duration = Duration::from_millis(300);
const SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";

/// A watched home: the files at `paths` (a path ending in `/` is a
/// folder) crawled, saved and watched.
struct Watched {
    home: TempFolder,
    _cache: TempFolder,
    watcher: Option<Watcher>,
    reader: LogReader,
    woken: Receiver<()>,
    seen: Vec<Change>,
}

impl Watched {
    fn new(paths: &[&str]) -> Self {
        Self::with_callback(paths, |_| {})
    }

    /// As [`Watched::new`], with `extra` run after each batch too.
    fn with_callback(paths: &[&str], extra: impl Fn(&Path) + Send + 'static) -> Self {
        let home = TempFolder::new().with(paths);
        let cache = TempFolder::new();
        let index_path = cache.path().join("index");
        crawl(home.path(), || false)
            .unwrap()
            .save(&index_path)
            .unwrap();
        let index = Index::open(&index_path).unwrap();
        let log_path = cache.path().join("changes");
        let log = ChangeLog::create(&log_path, index.crawled()).unwrap();
        let reader = LogReader::open(&log_path).unwrap();
        let (tell, woken) = mpsc::channel();
        let root = home.path().to_owned();
        let watcher = Watcher::start(&index, log, move || {
            tell.send(()).ok();
            extra(&root);
        })
        .unwrap();
        Self {
            home,
            _cache: cache,
            watcher: Some(watcher),
            reader,
            woken,
            seen: Vec::new(),
        }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.home.path().join(relative)
    }

    /// Waits until a change `wanted` accepts has been logged, and returns
    /// it.
    fn until(&mut self, wanted: impl Fn(&Change) -> bool) -> Change {
        let deadline = Instant::now() + WAIT;
        loop {
            self.seen.extend(self.reader.read_new().unwrap());
            if let Some(at) = self.seen.iter().position(&wanted) {
                return self.seen.remove(at);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(
                self.woken.recv_timeout(left).is_ok(),
                "no such change; saw {:?}",
                self.seen
            );
        }
    }

    /// Waits a moment and returns every change logged so far.
    fn settle(&mut self) -> Vec<Change> {
        while self.woken.recv_timeout(QUIET).is_ok() {}
        self.seen.extend(self.reader.read_new().unwrap());
        std::mem::take(&mut self.seen)
    }
}

fn added(path: &Path) -> impl Fn(&Change) -> bool + '_ {
    move |change| matches!(change, Change::Added { path: at, .. } if at == path)
}

fn removed(path: &Path) -> impl Fn(&Change) -> bool + '_ {
    move |change| matches!(change, Change::Removed { path: at } if at == path)
}

fn paths(changes: &[Change]) -> Vec<&Path> {
    changes
        .iter()
        .map(|change| match change {
            Change::Added { path, .. }
            | Change::Changed { path, .. }
            | Change::Removed { path } => path.as_path(),
            Change::Moved { to, .. } => to.as_path(),
            Change::Rescan { folder } => folder.as_path(),
        })
        .collect()
}

#[test]
fn a_new_file_is_added_with_its_details() {
    let mut watched = Watched::new(&["docs/"]);
    let path = watched.path("docs/new.txt");
    fs::write(&path, b"hello").unwrap();
    let Change::Added { details, .. } = watched.until(added(&path)) else {
        unreachable!()
    };
    assert_eq!(details.kind, Kind::File);
    let found = fs::metadata(&path).unwrap();
    assert_eq!(
        details.id.inode,
        std::os::unix::fs::MetadataExt::ino(&found)
    );
}

#[test]
fn a_written_file_is_changed_and_a_deleted_one_removed() {
    let mut watched = Watched::new(&["notes.txt"]);
    let path = watched.path("notes.txt");
    fs::write(&path, b"more").unwrap();
    let change = watched.until(|change| matches!(change, Change::Changed { .. }));
    let Change::Changed { path: at, details } = change else {
        unreachable!()
    };
    assert_eq!((at, details.size), (path.clone(), 4));
    fs::remove_file(&path).unwrap();
    watched.until(removed(&path));
}

#[test]
fn a_new_folder_is_added_with_everything_made_in_it() {
    let mut watched = Watched::new(&[]);
    let deep = watched.path("a/b/c");
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("file.txt"), b"").unwrap();
    for relative in ["a", "a/b", "a/b/c", "a/b/c/file.txt"] {
        let path = watched.path(relative);
        watched.until(added(&path));
    }
    // The new folders are watched too.
    let later = watched.path("a/b/later.txt");
    fs::write(&later, b"").unwrap();
    watched.until(added(&later));
}

#[test]
fn a_move_inside_home_is_one_move_and_its_watches_follow() {
    let mut watched = Watched::new(&["old/inner/file.txt", "dest/"]);
    let from = watched.path("old");
    let to = watched.path("dest/new");
    fs::rename(&from, &to).unwrap();
    let change = watched.until(|change| matches!(change, Change::Moved { .. }));
    assert_eq!(
        change,
        Change::Moved {
            from,
            to: to.clone()
        }
    );
    let later = to.join("inner/later.txt");
    fs::write(&later, b"").unwrap();
    watched.until(added(&later));
}

#[test]
fn moves_across_the_rules_add_or_remove() {
    let mut watched = Watched::new(&["shown.txt", ".ssh/"]);
    let shown = watched.path("shown.txt");
    let hidden = watched.path(".ssh/key");
    fs::rename(&shown, &hidden).unwrap();
    watched.until(removed(&shown));
    // .ssh isn't watched, so moving out of it looks like a new file.
    let back = watched.path("back.txt");
    fs::rename(&hidden, &back).unwrap();
    watched.until(added(&back));
}

#[test]
fn a_move_out_of_home_is_a_removal() {
    let mut watched = Watched::new(&["leaving/file.txt"]);
    let outside = TempFolder::new();
    let path = watched.path("leaving");
    fs::rename(&path, outside.path().join("gone")).unwrap();
    watched.until(removed(&path));
    // Its watches went with it: changes out there aren't logged.
    fs::write(outside.path().join("gone/after.txt"), b"").unwrap();
    assert_eq!(watched.settle(), []);
}

#[test]
fn what_the_rules_skip_is_not_logged() {
    let mut watched = Watched::new(&["project/"]);
    for folder in [".git", "node_modules", "__pycache__"] {
        let path = watched.path(&format!("project/{folder}/file"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"").unwrap();
    }
    fs::write(watched.path(".secret"), b"").unwrap();
    let changes = watched.settle();
    // Made, then written: added, then changed.
    let mut touched = paths(&changes);
    touched.dedup();
    assert_eq!(touched, [watched.path(".secret").as_path()]);
}

#[test]
fn a_folder_that_becomes_a_cache_is_removed() {
    let mut watched = Watched::new(&["project/"]);
    let target = watched.path("project/target");
    fs::create_dir(&target).unwrap();
    watched.until(added(&target));
    fs::write(target.join("CACHEDIR.TAG"), SIGNATURE).unwrap();
    watched.until(removed(&target));
    watched.settle();
    fs::write(target.join("build.o"), b"").unwrap();
    assert_eq!(watched.settle(), []);
}

#[test]
fn a_stray_cache_tag_changes_nothing() {
    let mut watched = Watched::new(&["project/"]);
    let tag = watched.path("project/CACHEDIR.TAG");
    fs::write(&tag, b"not a signature").unwrap();
    let changes = watched.settle();
    assert!(
        changes
            .iter()
            .all(|change| !matches!(change, Change::Removed { .. }))
    );
    assert!(paths(&changes).contains(&tag.as_path()));
}

#[test]
fn stopping_ends_the_thread() {
    let mut watched = Watched::new(&[]);
    watched.watcher.take().unwrap().stop().unwrap();
    fs::write(watched.path("after.txt"), b"").unwrap();
    assert_eq!(watched.settle(), []);
}

#[test]
fn a_file_moved_onto_the_way_to_an_allowed_place_is_not_added() {
    let mut watched = Watched::new(&[".local/bin/"]);
    let outside = TempFolder::new();
    let file = outside.path().join("note.txt");
    fs::write(&file, b"").unwrap();
    fs::rename(&file, watched.path(".local/note.txt")).unwrap();
    // A file that is kept, so the test knows the watcher has caught up.
    let shown = watched.path(".local/bin/tool");
    fs::write(&shown, b"").unwrap();
    watched.until(added(&shown));
    let changes = watched.settle();
    assert!(
        !paths(&changes).contains(&watched.path(".local/note.txt").as_path()),
        "{changes:?}"
    );
}

#[test]
fn a_marker_in_home_itself_removes_nothing() {
    let mut watched = Watched::new(&[]);
    let marker = watched.path("profiles.ini");
    fs::write(&marker, b"").unwrap();
    watched.until(added(&marker));
    let changes = watched.settle();
    let root = watched.home.path().to_owned();
    assert!(!changes.iter().any(removed(&root)), "{changes:?}");
}

#[test]
fn a_folder_that_cant_be_read_is_added_with_a_rescan() {
    use std::os::unix::fs::DirBuilderExt as _;
    const NO_ACCESS: u32 = 0o000;
    let mut watched = Watched::new(&[]);
    let locked = watched.path("locked");
    fs::DirBuilder::new()
        .mode(NO_ACCESS)
        .create(&locked)
        .unwrap();
    if fs::read_dir(&locked).is_ok() {
        // Running as root, which reads and watches it anyway.
        return;
    }
    watched.until(added(&locked));
    let rescan = Change::Rescan {
        folder: locked.clone(),
    };
    watched.until(|change| *change == rescan);
}

#[test]
fn a_failing_thread_is_reported_on_stop() {
    let mut watched = Watched::with_callback(&[], |root| {
        assert!(!root.join("boom").exists(), "on_change failed");
    });
    assert!(watched.watcher.as_ref().unwrap().is_running());
    fs::write(watched.path("boom"), b"").unwrap();
    watched.until(added(&watched.path("boom")));
    let deadline = Instant::now() + WAIT;
    while watched.watcher.as_ref().unwrap().is_running() {
        assert!(Instant::now() < deadline, "the thread didn't end");
        std::thread::yield_now();
    }
    assert!(watched.watcher.take().unwrap().stop().is_err());
}

#[test]
fn dropping_the_watcher_stops_it() {
    let mut watched = Watched::new(&[]);
    drop(watched.watcher.take());
    fs::write(watched.path("after.txt"), b"").unwrap();
    assert_eq!(watched.settle(), []);
}

#[test]
fn a_link_to_a_folder_moves_like_a_file() {
    let mut watched = Watched::new(&["docs/", ".local/bin/"]);
    let link = watched.path("docs-link");
    std::os::unix::fs::symlink(watched.path("docs"), &link).unwrap();
    watched.until(added(&link));
    // Files aren't kept on the way to an allowed place; a folder named
    // share would be, so following the link would get this wrong.
    let moved = watched.path(".local/share");
    fs::rename(&link, &moved).unwrap();
    watched.until(removed(&link));
}
