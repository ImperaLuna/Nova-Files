//! Crawling real temp folder trees, saving the index and opening it again:
//! what is indexed and what isn't, the depth-first layout, the details of
//! each row, cancelling, and replacing a saved index.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

use std::cell::Cell;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt as _;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _, symlink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use nova_files_index::{CrawlError, Flags, Index, Kind, OpenError, RowId, Stamp, crawl};

const NO_ACCESS: u32 = 0o000;
const PRIVATE_FILE: u32 = 0o600;
const PRIVATE_FOLDER: u32 = 0o700;
const MODE_BITS: u32 = 0o777;

/// A folder under the system temp folder, removed when dropped.
struct TempFolder(PathBuf);

impl TempFolder {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "nova-files-index-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    /// Makes the files at `paths` (and their folders); a path ending in
    /// `/` is a folder.
    fn with(self, paths: &[&str]) -> Self {
        for path in paths {
            let full = self.0.join(path);
            if path.ends_with('/') {
                fs::create_dir_all(&full).unwrap();
            } else {
                fs::create_dir_all(full.parent().unwrap()).unwrap();
                fs::write(&full, path.as_bytes()).unwrap();
            }
        }
        self
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

/// Crawls `home`, saves the index beside it and opens it again.
fn indexed(home: &Path) -> (Index, TempFolder) {
    let cache = TempFolder::new();
    let path = cache.path().join("index");
    crawl(home, || false).unwrap().save(&path).unwrap();
    (Index::open(&path).unwrap(), cache)
}

fn rows(index: &Index) -> impl Iterator<Item = RowId> {
    (0..index.len()).map(RowId)
}

/// Every indexed path except the crawled folder, relative to it.
fn relative_paths(index: &Index) -> BTreeSet<PathBuf> {
    rows(index)
        .skip(1)
        .map(|row| {
            let path = index.path(row).unwrap();
            path.strip_prefix(index.root()).unwrap().to_owned()
        })
        .collect()
}

fn find(index: &Index, relative: &str) -> RowId {
    let wanted = index.root().join(relative);
    rows(index)
        .find(|&row| index.path(row).unwrap() == wanted)
        .unwrap_or_else(|| panic!("{relative} isn't indexed"))
}

/// A home folder with something for each rule.
const HOME: &[&str] = &[
    "Documents/a.txt",
    "Documents/Taxes/2025.pdf",
    "Empty/",
    ".bashrc",
    ".ssh/id_ed25519",
    ".gnupg/pubring.kbx",
    ".mozilla/firefox/profiles.ini",
    ".var/app/org.example.App/data/file",
    ".config/app/settings.toml",
    ".config/app/GPUCache/data_0",
    ".config/chromium/Local State",
    ".config/chromium/Default/History",
    ".config/mozilla/firefox/profiles.ini",
    ".config/mozilla/firefox/abc.default/places.sqlite",
    ".local/bin/tool",
    ".local/share/applications/app.desktop",
    ".local/share/fonts/font.ttf",
    ".local/share/recently-used.xbel",
    ".local/share/Trash/files/old.txt",
    ".local/state/app/log",
    ".local/share/Steam/steamapps/common/Game/game.exe",
    ".local/share/Steam/steamapps/compatdata/1/pfx/system.reg",
    ".themes/Soft/index.theme",
    ".icons/Round/index.theme",
    "Projects/app/.gitignore",
    "Projects/app/.git/HEAD",
    "Projects/app/src/main.rs",
    "Projects/app/node_modules/dep/index.js",
    "Projects/app/__pycache__/mod.pyc",
    "Projects/app/target/CACHEDIR.TAG",
    "Projects/app/target/debug/app",
    "Projects/app/stray/CACHEDIR.TAG",
    "Games/prefix/drive_c/windows/win.ini",
    "Games/prefix/system.reg",
];

/// What of [`HOME`] is indexed, with a link to Documents.
const INDEXED: &[&str] = &[
    ".bashrc",
    ".config",
    ".config/app",
    ".config/app/settings.toml",
    ".config/mozilla",
    ".icons",
    ".icons/Round",
    ".icons/Round/index.theme",
    ".local",
    ".local/bin",
    ".local/bin/tool",
    ".local/share",
    ".local/share/Steam",
    ".local/share/Steam/steamapps",
    ".local/share/Steam/steamapps/common",
    ".local/share/Steam/steamapps/common/Game",
    ".local/share/Steam/steamapps/common/Game/game.exe",
    ".local/share/applications",
    ".local/share/applications/app.desktop",
    ".local/share/fonts",
    ".local/share/fonts/font.ttf",
    ".themes",
    ".themes/Soft",
    ".themes/Soft/index.theme",
    "Docs link",
    "Documents",
    "Documents/Taxes",
    "Documents/Taxes/2025.pdf",
    "Documents/a.txt",
    "Empty",
    "Games",
    "Projects",
    "Projects/app",
    "Projects/app/.gitignore",
    "Projects/app/src",
    "Projects/app/src/main.rs",
    "Projects/app/stray",
    "Projects/app/stray/CACHEDIR.TAG",
];

#[test]
fn home_is_indexed_without_hidden_folders_caches_and_profiles() {
    let home = TempFolder::new().with(HOME);
    fs::write(
        home.path().join("Projects/app/target/CACHEDIR.TAG"),
        b"Signature: 8a477f597d28d172789f06886806bc55\n# made by cargo\n",
    )
    .unwrap();
    symlink("Documents", home.path().join("Docs link")).unwrap();
    let (index, _cache) = indexed(home.path());
    let expected: BTreeSet<PathBuf> = INDEXED.iter().map(PathBuf::from).collect();
    assert_eq!(relative_paths(&index), expected);
    assert_eq!(
        index.entry(find(&index, "Docs link")).unwrap().kind,
        Kind::Link
    );
}

#[test]
fn rows_are_depth_first_with_each_folder_a_run() {
    let home = TempFolder::new().with(&["b/x/1", "b/x/2", "b/y", "a", "c/", "B"]);
    let (index, _cache) = indexed(home.path());
    let order: Vec<PathBuf> = rows(&index)
        .skip(1)
        .map(|row| {
            index
                .path(row)
                .unwrap()
                .strip_prefix(home.path())
                .unwrap()
                .to_owned()
        })
        .collect();
    let expected: Vec<PathBuf> = ["B", "a", "b", "b/x", "b/x/1", "b/x/2", "b/y", "c"]
        .iter()
        .map(PathBuf::from)
        .collect();
    assert_eq!(order, expected);
    for row in rows(&index) {
        let entry = index.entry(row).unwrap();
        let path = index.path(row).unwrap();
        for inner in rows(&index) {
            let inside = inner > row && inner < entry.end;
            assert_eq!(
                inside,
                index.path(inner).unwrap().starts_with(&path) && inner != row
            );
        }
        if let Some(parent) = entry.parent {
            assert_eq!(index.path(parent).unwrap(), path.parent().unwrap());
            assert_eq!(index.name(row).unwrap(), path.file_name().unwrap());
        }
    }
    assert_eq!(index.entry(RowId(0)).unwrap().parent, None);
    assert_eq!(index.entry(RowId(index.len())), None);
    assert_eq!(index.name(RowId(index.len())), None);
}

#[test]
fn rows_hold_the_files_details() {
    let home = TempFolder::new().with(&["notes/today.txt"]);
    let before = now();
    let (index, cache) = indexed(home.path());
    assert!(index.crawled().unwrap() >= before && index.crawled().unwrap() <= now());
    for (relative, kind) in [("notes", Kind::Folder), ("notes/today.txt", Kind::File)] {
        let entry = index.entry(find(&index, relative)).unwrap();
        let details = fs::symlink_metadata(home.path().join(relative)).unwrap();
        assert_eq!(entry.kind, kind);
        assert_eq!(entry.flags, Flags::default());
        assert_eq!(entry.id.device, details.dev());
        assert_eq!(entry.id.inode, details.ino());
        assert_eq!(entry.size, details.size());
        let modified = details.mtime() * 1_000_000_000 + details.mtime_nsec();
        assert_eq!(entry.modified, Some(Stamp(modified)));
        let created = details.created().ok().map(stamp);
        assert_eq!(entry.created, created);
    }
    assert_eq!(index.root(), home.path());
    assert_eq!(index.entry(RowId(0)).unwrap().kind, Kind::Folder);
    assert!(!index.is_empty());
    assert_eq!(
        index.bytes(),
        usize::try_from(fs::metadata(cache.path().join("index")).unwrap().len()).unwrap()
    );
}

#[test]
fn names_that_arent_utf8_survive() {
    let home = TempFolder::new();
    let name = OsString::from_vec(vec![b'n', 0xff, b'.', b't']);
    fs::write(home.path().join(&name), b"").unwrap();
    let (index, _cache) = indexed(home.path());
    assert_eq!(index.name(RowId(1)).unwrap(), name.as_os_str());
}

#[test]
fn a_folder_that_cant_be_read_is_indexed_empty() {
    let home = TempFolder::new().with(&["locked/secret.txt", "open.txt"]);
    let locked = home.path().join("locked");
    fs::set_permissions(&locked, fs::Permissions::from_mode(NO_ACCESS)).unwrap();
    if fs::read_dir(&locked).is_ok() {
        // Running as root, which reads it anyway.
        fs::set_permissions(&locked, fs::Permissions::from_mode(PRIVATE_FOLDER)).unwrap();
        return;
    }
    let (index, _cache) = indexed(home.path());
    fs::set_permissions(&locked, fs::Permissions::from_mode(PRIVATE_FOLDER)).unwrap();
    let row = find(&index, "locked");
    let entry = index.entry(row).unwrap();
    assert!(entry.flags.contains(Flags::UNREADABLE));
    assert_eq!(entry.end, RowId(row.0 + 1));
    find(&index, "open.txt");
}

#[test]
fn another_drive_is_indexed_but_not_entered() {
    // /dev holds mount points of other filesystems (pts, shm) on most
    // systems; without one there is nothing to test.
    let dev = Path::new("/dev");
    let device = fs::metadata(dev).unwrap().dev();
    let Some(mount) = fs::read_dir(dev).unwrap().flatten().find(|item| {
        item.file_type().is_ok_and(|kind| kind.is_dir())
            && !item.file_name().to_string_lossy().starts_with('.')
            && item.metadata().is_ok_and(|details| details.dev() != device)
            && fs::read_dir(item.path()).is_ok_and(|mut inside| inside.next().is_some())
    }) else {
        return;
    };
    let (index, _cache) = indexed(dev);
    let row = find(&index, &mount.file_name().to_string_lossy());
    let entry = index.entry(row).unwrap();
    assert!(entry.flags.contains(Flags::OTHER_DRIVE));
    assert_eq!(entry.end, RowId(row.0 + 1));
}

#[test]
fn the_crawl_asks_to_stop_before_every_read() {
    let home = TempFolder::new().with(&["a/1", "a/2", "a/CACHEDIR.TAG", "b"]);
    let asked = Cell::new(0);
    crawl(home.path(), || {
        asked.set(asked.get() + 1);
        false
    })
    .unwrap();
    // Once at the start, then for each of the folders home and a: once
    // before reading it and once per entry; and once before reading a's
    // CACHEDIR.TAG, which has no signature.
    assert_eq!(asked.get(), 1 + (1 + 2) + (1 + 3 + 1));
    for stop_at in 1..=asked.get() {
        let asked = Cell::new(0);
        let result = crawl(home.path(), || {
            asked.set(asked.get() + 1);
            asked.get() >= stop_at
        });
        assert_eq!(
            result.err(),
            Some(CrawlError::Cancelled),
            "stopped at {stop_at}"
        );
    }
}

#[test]
fn only_a_folder_can_be_crawled() {
    let home = TempFolder::new().with(&["file"]);
    assert_eq!(
        crawl(&home.path().join("file"), || false).err(),
        Some(CrawlError::NotAFolder)
    );
    assert_eq!(
        crawl(&home.path().join("missing"), || false).err(),
        Some(CrawlError::Unreadable(std::io::ErrorKind::NotFound))
    );
}

#[test]
fn saving_replaces_the_index_and_readers_keep_the_old_one() {
    let first = TempFolder::new().with(&["one"]);
    let second = TempFolder::new().with(&["one", "two"]);
    let cache = TempFolder::new();
    let path = cache.path().join("nested/index");
    crawl(first.path(), || false).unwrap().save(&path).unwrap();
    let old = Index::open(&path).unwrap();
    crawl(second.path(), || false).unwrap().save(&path).unwrap();
    let new = Index::open(&path).unwrap();
    assert_eq!((old.len(), old.root()), (2, first.path()));
    assert_eq!((new.len(), new.root()), (3, second.path()));
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & MODE_BITS;
    assert_eq!(mode(&path), PRIVATE_FILE);
    assert_eq!(mode(path.parent().unwrap()), PRIVATE_FOLDER);
    let left: Vec<_> = fs::read_dir(path.parent().unwrap()).unwrap().collect();
    assert_eq!(left.len(), 1, "only the index is left");
}

#[test]
fn a_stale_temp_file_is_replaced() {
    let home = TempFolder::new().with(&["one"]);
    let cache = TempFolder::new();
    let path = cache.path().join("index");
    fs::write(cache.path().join("index.new"), b"left by a crash").unwrap();
    crawl(home.path(), || false).unwrap().save(&path).unwrap();
    assert_eq!(Index::open(&path).unwrap().len(), 2);
    assert!(!cache.path().join("index.new").exists());
}

#[test]
fn saving_where_it_cant_write_keeps_nothing() {
    let home = TempFolder::new().with(&["one"]);
    let cache = TempFolder::new().with(&["blocker"]);
    // A file where the index's folder should be.
    let path = cache.path().join("blocker/index");
    assert!(crawl(home.path(), || false).unwrap().save(&path).is_err());
}

#[test]
fn opening_tells_why_there_is_no_index() {
    let cache = TempFolder::new().with(&["short", "other"]);
    fs::write(cache.path().join("other"), vec![b'x'; 4096]).unwrap();
    assert_eq!(
        Index::open(&cache.path().join("missing")).err(),
        Some(OpenError::Missing)
    );
    assert_eq!(
        Index::open(&cache.path().join("other")).err(),
        Some(OpenError::NotAnIndex)
    );
    assert_eq!(
        Index::open(&cache.path().join("short")).err(),
        Some(OpenError::Damaged("shorter than a header"))
    );
    assert!(matches!(
        Index::open(cache.path()).err(),
        Some(OpenError::Failed(_))
    ));
}

fn stamp(time: SystemTime) -> Stamp {
    Stamp(i64::try_from(time.duration_since(UNIX_EPOCH).unwrap().as_nanos()).unwrap())
}

fn now() -> Stamp {
    stamp(SystemTime::now())
}
