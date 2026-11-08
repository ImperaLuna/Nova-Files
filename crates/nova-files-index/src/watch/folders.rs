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
use crate::mounts::{MountCheck as _, SystemMounts};
use crate::policy::{self, Zone};
use crate::reader::Index;
use crate::{Details, Flags, Kind, RowId};

/// What a watch reports: everything that changes what a folder holds or
/// an entry's details. Links aren't followed, the target is a folder, and
/// a deleted file still open elsewhere stops reporting.
const WATCHED: WatchFlags = WatchFlags::CREATE
    .union(WatchFlags::DELETE)
    .union(WatchFlags::MOVED_FROM)
    .union(WatchFlags::MOVED_TO)
    .union(WatchFlags::CLOSE_WRITE)
    .union(WatchFlags::ATTRIB)
    .union(WatchFlags::MOVE_SELF)
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
    /// What the event says it was: by the time it is paired, the entry
    /// may have moved on, so the disk can't tell.
    is_folder: bool,
}

/// Whether the watcher is being stopped, so a long read of a new tree can
/// end early.
pub type Stopping = Box<dyn Fn() -> bool + Send>;

pub struct Folders {
    inotify: OwnedFd,
    root: PathBuf,
    by_wd: HashMap<i32, Watched>,
    by_path: HashMap<PathBuf, i32>,
    stopping: Stopping,
}

impl Folders {
    pub fn new(inotify: OwnedFd, root: PathBuf, stopping: Stopping) -> Self {
        Self {
            inotify,
            root,
            by_wd: HashMap::new(),
            by_path: HashMap::new(),
            stopping,
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
            let Some(entry) = index.entry(row) else {
                continue;
            };
            let not_read =
                entry.flags.contains(Flags::UNREADABLE) || entry.flags.contains(Flags::OTHER_DRIVE);
            if entry.kind != Kind::Folder || not_read {
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
            self.gone(gone, &mut changes);
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
            let Some(gone) = self.by_wd.remove(&event.wd) else {
                return;
            };
            // A folder moved over this one may hold its path by now.
            if self.by_path.get(&gone.path) == Some(&event.wd) {
                self.by_path.remove(&gone.path);
            }
            if gone.path == self.root {
                // Home itself went, so nothing watches what takes its place.
                changes.push(Change::Rescan {
                    folder: self.root.clone(),
                });
            }
            return;
        }
        if event.mask.contains(ReadFlags::MOVE_SELF) {
            // A folder's own move is seen from its parent, except home's.
            if self.by_path.get(&self.root) == Some(&event.wd) {
                // Its watches now see wherever it went, not what stands
                // at its path.
                let root = self.root.clone();
                self.forget(&root);
                changes.push(Change::Rescan { folder: root });
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
        if !mask.contains(ReadFlags::MOVED_FROM) {
            // Something new at a path that moved away earlier in this
            // batch: that move left the tree, and must end before this
            // takes its place.
            self.leave_from(&path, leaving, changes);
        }
        if mask.contains(ReadFlags::MOVED_FROM) {
            leaving.insert(
                event.cookie,
                Leaving {
                    path,
                    kept,
                    is_folder,
                },
            );
        } else if mask.contains(ReadFlags::MOVED_TO) {
            match leaving.remove(&event.cookie) {
                Some(from) => {
                    if from.is_folder {
                        self.leave_under(&from.path, leaving, changes);
                    }
                    self.moved(from, &folder, path, changes);
                }
                None if kept => self.add(&folder.zone, name, path, changes),
                None => {}
            }
            self.check_marker(&folder, name, changes);
        } else if mask.contains(ReadFlags::CREATE) {
            if kept {
                self.add(&folder.zone, name, path, changes);
            }
            self.check_marker(&folder, name, changes);
        } else if mask.contains(ReadFlags::DELETE) {
            self.forget_if_folder(&path, is_folder);
            if kept {
                changes.push(Change::Removed { path });
            }
        } else if mask.intersects(ReadFlags::CLOSE_WRITE.union(ReadFlags::ATTRIB)) {
            if kept && is_folder && self.can_enter_unwatched(&folder.zone, name, &path) {
                // Most likely a folder that couldn't be read now can be.
                self.add(&folder.zone, name, path, changes);
            } else if kept {
                changed(path, changes);
            }
            self.check_marker(&folder, name, changes);
        }
    }

    /// Whether the folder at `path`, named `name` in a folder of `zone`,
    /// is one the rules enter but that has no watch, and isn't another
    /// drive's (which the crawl doesn't read).
    fn can_enter_unwatched(&self, zone: &Zone, name: &OsStr, path: &Path) -> bool {
        !self.by_path.contains_key(path)
            && policy::enters(zone, name).is_some()
            && !SystemMounts::default().is_mount_root(path)
    }

    /// Ends the move away from `path` still waiting for its second half,
    /// if there is one: it left the tree.
    fn leave_from(
        &mut self,
        path: &Path,
        leaving: &mut HashMap<u32, Leaving>,
        changes: &mut Vec<Change>,
    ) {
        let Some(&cookie) = leaving
            .iter()
            .find_map(|(cookie, gone)| (gone.path == path).then_some(cookie))
        else {
            return;
        };
        if let Some(gone) = leaving.remove(&cookie) {
            self.gone(gone, changes);
        }
    }

    /// Ends the moves away from inside `folder` still pending, before
    /// `folder` itself moves: they left from where it was, and must not
    /// move with it.
    fn leave_under(
        &mut self,
        folder: &Path,
        leaving: &mut HashMap<u32, Leaving>,
        changes: &mut Vec<Change>,
    ) {
        let under: Vec<u32> = leaving
            .iter()
            .filter(|(_, gone)| gone.path.starts_with(folder))
            .map(|(&cookie, _)| cookie)
            .collect();
        for cookie in under {
            if let Some(gone) = leaving.remove(&cookie) {
                self.gone(gone, changes);
            }
        }
    }

    /// A move away from `gone.path` that never arrived inside the tree.
    fn gone(&mut self, gone: Leaving, changes: &mut Vec<Change>) {
        self.forget_if_folder(&gone.path, gone.is_folder);
        if gone.kept {
            changes.push(Change::Removed { path: gone.path });
        }
    }

    /// The second half of a move from `from` to `to`, in `folder`.
    fn moved(&mut self, from: Leaving, folder: &Watched, to: PathBuf, changes: &mut Vec<Change>) {
        let name = to.file_name().unwrap_or_default().to_owned();
        let name = name.as_os_str();
        // Links aren't followed: the kernel marks a link to a folder as no
        // folder.
        let is_folder = from.is_folder;
        let kept = policy::keeps(&folder.zone, name, is_folder);
        if is_folder && self.swapped(&from.path, &to) {
            // Exchanged with the folder at `to` (`RENAME_EXCHANGE`): two
            // moves that each land on the other, so both are read again.
            self.read_again(&[from.path, to], changes);
            return;
        }
        match (from.kept, kept) {
            (true, true) if !is_folder => {
                changes.push(Change::Moved {
                    from: from.path,
                    to: to.clone(),
                });
                // The file may have been made under its old name too late
                // to be seen there, or written since.
                self.add(&folder.zone, name, to, changes);
            }
            (true, true)
                if self.zone_of_watch(&from.path) == policy::enters(&folder.zone, name) =>
            {
                self.repath(&from.path, &to);
                changes.push(Change::Moved {
                    from: from.path,
                    to,
                });
            }
            (true, true) => {
                // Not watched at its old path (made and moved before its
                // events were read), or now under other rules (out of a
                // hidden place, say): what it held there says nothing of
                // what is indexed here, so it is read again.
                self.forget_if_folder(&from.path, is_folder);
                changes.push(Change::Removed { path: from.path });
                self.add(&folder.zone, name, to, changes);
            }
            (true, false) => {
                self.forget_if_folder(&from.path, is_folder);
                changes.push(Change::Removed { path: from.path });
            }
            (false, true) => {
                self.forget_if_folder(&from.path, is_folder);
                self.add(&folder.zone, name, to, changes);
            }
            (false, false) => self.forget_if_folder(&from.path, is_folder),
        }
    }

    /// The zone of the watch on the folder at `path`, if it has one.
    /// Whether a folder moved from `from` to `to` swapped places with the
    /// one there: `to` is still watched as another folder, and a folder
    /// still stands at `from`.
    fn swapped(&self, from: &Path, to: &Path) -> bool {
        let (Some(moving), Some(there)) = (self.by_path.get(from), self.by_path.get(to)) else {
            return false;
        };
        moving != there && fs::symlink_metadata(from).is_ok_and(|details| details.is_dir())
    }

    /// Removes what was indexed at `paths` and reads them anew. Every
    /// watch goes before any is added, since each path now holds a folder
    /// another watch was on.
    fn read_again(&mut self, paths: &[PathBuf], changes: &mut Vec<Change>) {
        for path in paths {
            self.forget(path);
            changes.push(Change::Removed { path: path.clone() });
        }
        for path in paths {
            let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
                continue;
            };
            if let Some(zone) = self.zone_of_watch(parent) {
                self.add(&zone, name, path.clone(), changes);
            }
        }
    }

    fn zone_of_watch(&self, path: &Path) -> Option<Zone> {
        let wd = self.by_path.get(path)?;
        self.by_wd.get(wd).map(|watched| watched.zone.clone())
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
            if (self.stopping)() {
                return;
            }
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
        if SystemMounts::default().is_mount_root(&path) {
            // Another drive: listed, never entered, as the crawl does.
            let details = crawl::facts(details, Kind::Folder);
            changes.push(Change::Added {
                path,
                details: Details {
                    flags: details.flags.with(Flags::OTHER_DRIVE),
                    ..details
                },
            });
            return Vec::new();
        }
        changes.extend(self.watch(path.clone(), zone.clone()));
        let children = match crawl::read_children(&path, &self.stopping) {
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
                // The kernel hands back the watch a folder already has: one
                // moved here before its old path's events were read. Its
                // old path must not keep it, or its leaving drops it.
                if let Some(old) = self.by_wd.get(&wd)
                    && old.path != path
                    && self.by_path.get(&old.path) == Some(&wd)
                {
                    self.by_path.remove(&old.path);
                }
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

    /// Drops the watches under `path` when it was a folder: a file has
    /// none, and looking costs a pass over every watch.
    fn forget_if_folder(&mut self, path: &Path, is_folder: bool) {
        if is_folder {
            self.forget(path);
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
mod tests;
