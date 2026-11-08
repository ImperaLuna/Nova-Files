//! The watched folders, and turning inotify events into [`Change`]s.
//!
//! Each watch remembers its folder's path and `policy` zone, so an event
//! names a whole path and the rules decide whether it is indexed. A new
//! folder is watched before it is read, at every level, so nothing created
//! inside it in between is missed; anything seen both ways is added twice,
//! which is harmless since [`Change::Added`] means "now looks like this".
//!
//! A move inside the watched tree arrives as a `MOVED_FROM` and a
//! `MOVED_TO` with the same cookie. Paired within a batch, it is one
//! [`Change::Moved`], and the watches under it learn their new paths. A
//! `MOVED_FROM` left unpaired at the end of a batch moved out of the tree
//! (or to a folder not indexed): a removal. A lone `MOVED_TO` moved in: an
//! addition. A move that changes whether the rules keep the entry (into a
//! hidden folder, say) is a removal or an addition.
//!
//! When a marker file (`policy::is_marker`) appears, the folder is checked
//! again; one that turned into a cache or a profile is removed, and its
//! watches with it. A queue overflow means events were lost: everything
//! needs a rescan.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};

use rustix::fs::inotify::{self, ReadFlags, WatchFlags};
use rustix::io::Errno;

use crate::changes::Change;
use crate::crawl::{self, Stop};
use crate::policy::{self, Zone};
use crate::reader::Index;
use crate::{Flags, Kind, RowId};

mod catch_up;

/// What a watch reports: everything that changes what a folder holds or
/// an entry's details. Links aren't followed, the target is a folder, and
/// a deleted file still open elsewhere stops reporting.
const WATCHED: WatchFlags = WatchFlags::CREATE
    .union(WatchFlags::DELETE)
    .union(WatchFlags::MOVED_FROM)
    .union(WatchFlags::MOVED_TO)
    .union(WatchFlags::CLOSE_WRITE)
    .union(WatchFlags::ATTRIB)
    .union(WatchFlags::DONT_FOLLOW)
    .union(WatchFlags::ONLYDIR)
    .union(WatchFlags::EXCL_UNLINK);

/// One event, copied out of the kernel's buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub wd: i32,
    pub mask: ReadFlags,
    pub cookie: u32,
    pub name: Option<OsString>,
}

/// A watched folder.
#[derive(Debug, Clone)]
struct Watched {
    path: PathBuf,
    zone: Zone,
}

/// The first half of a move, waiting for its second.
struct Leaving {
    path: PathBuf,
    kept: bool,
}

pub struct Folders {
    inotify: OwnedFd,
    root: PathBuf,
    by_wd: HashMap<i32, Watched>,
    by_path: HashMap<PathBuf, i32>,
}

impl Folders {
    pub fn new(inotify: OwnedFd, root: PathBuf) -> Self {
        Self {
            inotify,
            root,
            by_wd: HashMap::new(),
            by_path: HashMap::new(),
        }
    }

    pub const fn fd(&self) -> &OwnedFd {
        &self.inotify
    }

    /// Watches every folder `index` holds that was read; returns a rescan
    /// for each one that couldn't be watched.
    pub fn watch_index(&mut self, index: &Index) -> Vec<Change> {
        let mut unwatched = Vec::new();
        for row in (0..index.rows()).map(RowId) {
            if !index.entry(row).is_some_and(|entry| was_read(&entry)) {
                continue;
            }
            let Some(path) = index.path(row) else {
                continue;
            };
            let zone = path.strip_prefix(&self.root).ok().and_then(policy::zone_of);
            if let Some(zone) = zone {
                unwatched.extend(self.watch(path, zone));
            }
        }
        unwatched
    }

    /// The changes a batch of events means.
    pub fn handle(&mut self, events: &[Event]) -> Vec<Change> {
        let mut changes = Vec::new();
        let mut leaving: HashMap<u32, Leaving> = HashMap::new();
        for event in events {
            self.handle_one(event, &mut leaving, &mut changes);
        }
        for (_, gone) in leaving {
            self.forget(&gone.path);
            if gone.kept {
                changes.push(Change::Removed { path: gone.path });
            }
        }
        changes
    }

    fn handle_one(
        &mut self,
        event: &Event,
        leaving: &mut HashMap<u32, Leaving>,
        changes: &mut Vec<Change>,
    ) {
        if event.mask.contains(ReadFlags::QUEUE_OVERFLOW) {
            changes.push(Change::Rescan {
                folder: self.root.clone(),
            });
            return;
        }
        if event.mask.contains(ReadFlags::IGNORED) {
            if let Some(gone) = self.by_wd.remove(&event.wd) {
                self.by_path.remove(&gone.path);
            }
            return;
        }
        let (Some(folder), Some(name)) = (self.by_wd.get(&event.wd).cloned(), &event.name) else {
            return;
        };
        let path = folder.path.join(name);
        let is_folder = event.mask.contains(ReadFlags::ISDIR);
        let kept = policy::keeps(&folder.zone, name, is_folder);
        let mask = event.mask;
        if mask.contains(ReadFlags::MOVED_FROM) {
            leaving.insert(event.cookie, Leaving { path, kept });
        } else if mask.contains(ReadFlags::MOVED_TO) {
            match leaving.remove(&event.cookie) {
                Some(from) => self.moved(from, &folder, path, changes),
                None if kept => self.add(&folder.zone, name, path, changes),
                None => {}
            }
        } else if mask.contains(ReadFlags::CREATE) {
            if kept {
                self.add(&folder.zone, name, path, changes);
            }
            self.check_marker(&folder, name, changes);
        } else if mask.contains(ReadFlags::DELETE) {
            self.forget(&path);
            if kept {
                changes.push(Change::Removed { path });
            }
        } else if mask.intersects(ReadFlags::CLOSE_WRITE.union(ReadFlags::ATTRIB)) {
            if kept {
                changed(path, changes);
            }
            self.check_marker(&folder, name, changes);
        }
    }

    /// The second half of a move from `from` to `to`, in `folder`.
    fn moved(&mut self, from: Leaving, folder: &Watched, to: PathBuf, changes: &mut Vec<Change>) {
        let name = to.file_name().unwrap_or_default().to_owned();
        let name = name.as_os_str();
        // Links aren't followed: a link to a folder is kept like a file.
        let is_folder = fs::symlink_metadata(&to).is_ok_and(|details| details.is_dir());
        let kept = policy::keeps(&folder.zone, name, is_folder);
        match (from.kept, kept) {
            (true, true) => {
                self.repath(&from.path, &to);
                changes.push(Change::Moved {
                    from: from.path,
                    to,
                });
            }
            (true, false) => {
                self.forget(&from.path);
                changes.push(Change::Removed { path: from.path });
            }
            (false, true) => {
                self.forget(&from.path);
                self.add(&folder.zone, name, to, changes);
            }
            (false, false) => self.forget(&from.path),
        }
    }

    /// Adds the entry at `path`, named `name` in a folder of `zone`; a
    /// folder with everything in it, each level watched before it is read.
    fn add(&mut self, zone: &Zone, name: &OsStr, path: PathBuf, changes: &mut Vec<Change>) {
        let Ok(details) = fs::symlink_metadata(&path) else {
            // Gone again already.
            return;
        };
        if !details.is_dir() {
            let kind = if details.is_symlink() {
                Kind::Link
            } else {
                Kind::File
            };
            changes.push(Change::Added {
                path,
                details: crawl::facts(&details, kind),
            });
            return;
        }
        let Some(inner) = policy::enters(zone, name) else {
            return;
        };
        let mut pending = vec![(path, inner, details)];
        while let Some((path, zone, details)) = pending.pop() {
            pending.extend(self.add_folder(path, &zone, &details, changes));
        }
    }

    /// Watches and reads one new folder, adding it and its files; returns
    /// its subfolders, for the caller to add the same way.
    fn add_folder(
        &mut self,
        path: PathBuf,
        zone: &Zone,
        details: &fs::Metadata,
        changes: &mut Vec<Change>,
    ) -> Vec<(PathBuf, Zone, fs::Metadata)> {
        changes.extend(self.watch(path.clone(), zone.clone()));
        let children = match crawl::read_children(&path, &|| false) {
            Ok(children) => children,
            Err(Stop::Cancelled | Stop::Gone | Stop::Failed(_)) => {
                // Gone already, or unreadable: indexed without contents.
                changes.push(Change::Added {
                    details: crawl::facts(details, Kind::Folder),
                    path,
                });
                return Vec::new();
            }
        };
        if is_marked(&path, &children) {
            self.forget(&path);
            return Vec::new();
        }
        changes.push(Change::Added {
            path: path.clone(),
            details: crawl::facts(details, Kind::Folder),
        });
        let mut folders = Vec::new();
        for child in children {
            let Ok(inner) = child.details else {
                continue;
            };
            let at = path.join(&child.name);
            if inner.is_dir() {
                if let Some(inner_zone) = policy::enters(zone, &child.name) {
                    folders.push((at, inner_zone, inner));
                }
            } else if policy::keeps_file(zone) {
                let kind = if inner.is_symlink() {
                    Kind::Link
                } else {
                    Kind::File
                };
                changes.push(Change::Added {
                    path: at,
                    details: crawl::facts(&inner, kind),
                });
            }
        }
        folders
    }

    /// When `name` can mark `folder` as not worth indexing, checks it, and
    /// removes it if it now is.
    fn check_marker(&mut self, folder: &Watched, name: &OsStr, changes: &mut Vec<Change>) {
        if !policy::is_marker(name) || folder.path == self.root {
            return;
        }
        let Ok(children) = crawl::read_children(&folder.path, &|| false) else {
            return;
        };
        if is_marked(&folder.path, &children) {
            self.forget(&folder.path);
            changes.push(Change::Removed {
                path: folder.path.clone(),
            });
        }
    }

    /// Watches the folder at `path`; a rescan of it when it can't be.
    fn watch(&mut self, path: PathBuf, zone: Zone) -> Option<Change> {
        match inotify::add_watch(&self.inotify, &path, WATCHED) {
            Ok(wd) => {
                self.by_path.insert(path.clone(), wd);
                self.by_wd.insert(wd, Watched { path, zone });
                None
            }
            // Gone, or not a folder any more: its own event says so.
            Err(Errno::NOENT | Errno::NOTDIR) => None,
            // Over the watch limit, or not readable.
            Err(_) => Some(Change::Rescan { folder: path }),
        }
    }

    /// Drops the watches on `path` and everything under it.
    fn forget(&mut self, path: &Path) {
        let under: Vec<PathBuf> = self
            .by_path
            .keys()
            .filter(|watched| watched.starts_with(path))
            .cloned()
            .collect();
        for watched in under {
            if let Some(wd) = self.by_path.remove(&watched) {
                self.by_wd.remove(&wd);
                // The kernel may have dropped it already.
                inotify::remove_watch(&self.inotify, wd).ok();
            }
        }
    }

    /// The watches under `from` now sit under `to`, with the zones there.
    fn repath(&mut self, from: &Path, to: &Path) {
        let moved: Vec<(PathBuf, i32)> = self
            .by_path
            .iter()
            .filter(|(watched, _)| watched.starts_with(from))
            .map(|(watched, &wd)| (watched.clone(), wd))
            .collect();
        for (old, wd) in moved {
            self.by_path.remove(&old);
            let Ok(rest) = old.strip_prefix(from) else {
                continue;
            };
            let new = if rest.as_os_str().is_empty() {
                to.to_owned()
            } else {
                to.join(rest)
            };
            let zone = new.strip_prefix(&self.root).ok().and_then(policy::zone_of);
            if let Some(zone) = zone {
                self.by_path.insert(new.clone(), wd);
                self.by_wd.insert(wd, Watched { path: new, zone });
            } else {
                self.by_wd.remove(&wd);
                inotify::remove_watch(&self.inotify, wd).ok();
            }
        }
    }
}

/// Whether the crawl read this folder: the ones to watch and compare.
const fn was_read(entry: &crate::Entry) -> bool {
    matches!(entry.kind, Kind::Folder)
        && !entry.flags.contains(Flags::UNREADABLE)
        && !entry.flags.contains(Flags::OTHER_DRIVE)
}

/// A file at `path` changed: its new details, unless it is gone again.
fn changed(path: PathBuf, changes: &mut Vec<Change>) {
    let Ok(details) = fs::symlink_metadata(&path) else {
        return;
    };
    let kind = if details.is_dir() {
        Kind::Folder
    } else if details.is_symlink() {
        Kind::Link
    } else {
        Kind::File
    };
    changes.push(Change::Changed {
        path,
        details: crawl::facts(&details, kind),
    });
}

/// Whether the folder at `path`, holding `children`, says it isn't worth
/// indexing, by the same rules as the crawl.
fn is_marked(path: &Path, children: &[crawl::Child]) -> bool {
    let names: Vec<OsString> = children.iter().map(|child| child.name.clone()).collect();
    policy::marked(&names)
        || (names.iter().any(|name| name == policy::CACHE_TAG)
            && crawl::has_cache_signature(&path.join(policy::CACHE_TAG)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustix::fs::inotify::CreateFlags;

    fn folders(root: &Path) -> Folders {
        let fd = inotify::init(CreateFlags::CLOEXEC.union(CreateFlags::NONBLOCK)).unwrap();
        Folders::new(fd, root.to_owned())
    }

    #[test]
    fn an_overflow_asks_for_a_rescan_of_everything() {
        let mut folders = folders(Path::new("/home/me"));
        let overflow = Event {
            wd: -1,
            mask: ReadFlags::QUEUE_OVERFLOW,
            cookie: 0,
            name: None,
        };
        assert_eq!(
            folders.handle(&[overflow]),
            [Change::Rescan {
                folder: PathBuf::from("/home/me")
            }]
        );
    }

    #[test]
    fn events_from_unknown_watches_are_passed_over() {
        let mut folders = folders(Path::new("/home/me"));
        let stray = Event {
            wd: 7,
            mask: ReadFlags::CREATE,
            cookie: 0,
            name: Some(OsString::from("a.txt")),
        };
        assert_eq!(folders.handle(&[stray]), []);
    }

    #[test]
    fn a_watch_the_kernel_dropped_is_forgotten() {
        let root =
            std::env::temp_dir().join(format!("nova-files-watch-ignored-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let mut folders = folders(&root);
        assert_eq!(folders.watch(root.clone(), Zone::Home), None);
        let wd = folders.by_path[&root];
        let ignored = Event {
            wd,
            mask: ReadFlags::IGNORED,
            cookie: 0,
            name: None,
        };
        folders.handle(&[ignored]);
        assert!(folders.by_wd.is_empty());
        assert!(folders.by_path.is_empty());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn folders_not_read_by_the_crawl_are_not_watched() {
        use crate::mounts::MountCheck;
        struct DriveIsMounted(PathBuf);
        impl MountCheck for DriveIsMounted {
            fn is_mount_root(&self, folder: &Path) -> bool {
                folder == self.0
            }
        }
        let root =
            std::env::temp_dir().join(format!("nova-files-watch-drive-{}", std::process::id()));
        fs::create_dir_all(root.join("drive/inside")).unwrap();
        fs::create_dir_all(root.join("plain")).unwrap();
        let built = crawl::crawl_with(&root, DriveIsMounted(root.join("drive")), || false).unwrap();
        let saved = root.with_extension("index");
        built.save(&saved).unwrap();
        let index = Index::open(&saved).unwrap();
        let mut folders = folders(&root);
        assert_eq!(folders.watch_index(&index), []);
        let mut watched: Vec<&PathBuf> = folders.by_path.keys().collect();
        watched.sort();
        assert_eq!(watched, [&root, &root.join("plain")]);
        fs::remove_file(&saved).ok();
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_folder_that_cant_be_watched_gets_a_rescan() {
        let mut folders = folders(Path::new("/"));
        let missing = PathBuf::from("/nowhere/at/all");
        assert_eq!(folders.watch(missing, Zone::Home), None);
        // A file isn't a folder: watching it is refused as gone.
        let file =
            std::env::temp_dir().join(format!("nova-files-watch-file-{}", std::process::id()));
        fs::write(&file, b"").unwrap();
        assert_eq!(folders.watch(file.clone(), Zone::Home), None);
        fs::remove_file(&file).ok();
    }
}
