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
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rustix::fs::inotify::{self, ReadFlags, WatchFlags};
use rustix::io::Errno;

use crate::changes::Change;
use crate::crawl::{self, Stop};
use crate::policy::{self, Zone};
use crate::reader::Index;
use crate::{Flags, Kind, RowId};

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
    unwatched: Arc<AtomicUsize>,
}

impl Folders {
    pub fn new(inotify: OwnedFd, root: PathBuf, unwatched: Arc<AtomicUsize>) -> Self {
        Self {
            inotify,
            root,
            by_wd: HashMap::new(),
            by_path: HashMap::new(),
            unwatched,
        }
    }

    pub const fn fd(&self) -> &OwnedFd {
        &self.inotify
    }

    /// Watches every folder `index` holds that was read.
    pub fn watch_index(&mut self, index: &Index) {
        let skipped = Flags::UNREADABLE.with(Flags::OTHER_DRIVE);
        for row in (0..index.rows()).map(RowId) {
            let Some(entry) = index.entry(row) else {
                continue;
            };
            if entry.kind != Kind::Folder || entry.flags.0 & skipped.0 != 0 {
                continue;
            }
            let Some(path) = index.path(row) else {
                continue;
            };
            let zone = path.strip_prefix(&self.root).ok().and_then(policy::zone_of);
            if let Some(zone) = zone {
                self.watch(path, zone);
            }
        }
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
        } else if mask.intersects(ReadFlags::CLOSE_WRITE | ReadFlags::ATTRIB) {
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
        let kept = policy::keeps(&folder.zone, name, to.is_dir());
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
        self.watch(path.clone(), zone.clone());
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

    fn watch(&mut self, path: PathBuf, zone: Zone) {
        let flags = WatchFlags::CREATE
            | WatchFlags::DELETE
            | WatchFlags::MOVED_FROM
            | WatchFlags::MOVED_TO
            | WatchFlags::CLOSE_WRITE
            | WatchFlags::ATTRIB
            | WatchFlags::DONT_FOLLOW
            | WatchFlags::ONLYDIR
            | WatchFlags::EXCL_UNLINK;
        match inotify::add_watch(&self.inotify, &path, flags) {
            Ok(wd) => {
                self.by_path.insert(path.clone(), wd);
                self.by_wd.insert(wd, Watched { path, zone });
            }
            Err(Errno::NOSPC) => {
                self.unwatched.fetch_add(1, Ordering::Relaxed);
            }
            // Gone, or not a folder any more: its own event says so.
            Err(_) => {}
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
        let fd = inotify::init(CreateFlags::CLOEXEC | CreateFlags::NONBLOCK).unwrap();
        Folders::new(fd, root.to_owned(), Arc::new(AtomicUsize::new(0)))
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
        assert!(folders.handle(&[stray]).is_empty());
    }

    #[test]
    fn a_watch_the_kernel_dropped_is_forgotten() {
        let root =
            std::env::temp_dir().join(format!("nova-files-watch-ignored-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let mut folders = folders(&root);
        folders.watch(root.clone(), Zone::Home);
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
}
