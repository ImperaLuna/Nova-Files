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

mod catch_up;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub wd: i32,
    pub mask: ReadFlags,
    pub cookie: u32,
    pub name: Option<OsString>,
}

#[derive(Debug, Clone)]
struct Watched {
    path: PathBuf,
    zone: Zone,
}

struct Leaving {
    path: PathBuf,
    kept: bool,
    is_folder: bool,
}

pub type Stopping = Box<dyn Fn() -> bool + Send>;

pub struct Folders {
    inotify: OwnedFd,
    root: PathBuf,
    by_wd: HashMap<i32, Watched>,
    by_path: HashMap<PathBuf, i32>,
    vanished: Vec<PathBuf>,
    stopping: Stopping,
}

impl Folders {
    pub fn new(inotify: OwnedFd, root: PathBuf, stopping: Stopping) -> Self {
        Self {
            inotify,
            root,
            by_wd: HashMap::new(),
            by_path: HashMap::new(),
            vanished: Vec::new(),
            stopping,
        }
    }

    pub const fn fd(&self) -> &OwnedFd {
        &self.inotify
    }

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

    pub fn handle(&mut self, events: &[Event]) -> Vec<Change> {
        let mut changes = Vec::new();
        let mut leaving: HashMap<u32, Leaving> = HashMap::new();
        self.vanished.clear();
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
            if self.by_path.get(&gone.path) == Some(&event.wd) {
                self.by_path.remove(&gone.path);
            }
            if gone.path == self.root {
                changes.push(Change::Rescan {
                    folder: self.root.clone(),
                });
            }
            return;
        }
        if event.mask.contains(ReadFlags::MOVE_SELF) {
            if self.by_path.get(&self.root) == Some(&event.wd) {
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
            self.leave_under(&path, leaving, changes);
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
                self.add(&folder.zone, name, path, changes);
            } else if kept {
                self.changed(path, changes);
            }
            self.check_marker(&folder, name, changes);
        }
    }

    fn can_enter_unwatched(&self, zone: &Zone, name: &OsStr, path: &Path) -> bool {
        !self.by_path.contains_key(path)
            && policy::enters(zone, name).is_some()
            && !SystemMounts::default().is_mount_root(path)
    }

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

    fn gone(&mut self, gone: Leaving, changes: &mut Vec<Change>) {
        self.forget_if_folder(&gone.path, gone.is_folder);
        if gone.kept {
            changes.push(Change::Removed { path: gone.path });
        }
    }

    fn moved(&mut self, from: Leaving, folder: &Watched, to: PathBuf, changes: &mut Vec<Change>) {
        let name = to.file_name().unwrap_or_default().to_owned();
        let name = name.as_os_str();
        let is_folder = from.is_folder;
        let kept = policy::keeps(&folder.zone, name, is_folder);
        if fs::symlink_metadata(&from.path).is_ok() {
            self.read_again(&[from.path, to], changes);
            return;
        }
        match (from.kept, kept) {
            (true, true) if !is_folder => {
                changes.push(Change::Moved {
                    from: from.path,
                    to: to.clone(),
                });
                self.add(&folder.zone, name, to, changes);
            }
            (true, true)
                if self.zone_of_watch(&from.path) == policy::enters(&folder.zone, name) =>
            {
                self.repath(&from.path, &to);
                let arrived: Vec<PathBuf> = self
                    .vanished
                    .iter()
                    .filter_map(|lost| Some(to.join(lost.strip_prefix(&from.path).ok()?)))
                    .collect();
                changes.push(Change::Moved {
                    from: from.path,
                    to,
                });
                for path in arrived {
                    self.add_again(&path, changes);
                }
            }
            (true, true) => {
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

    fn changed(&mut self, path: PathBuf, changes: &mut Vec<Change>) {
        let Ok(details) = fs::symlink_metadata(&path) else {
            self.vanished.push(path);
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

    fn add_again(&mut self, path: &Path, changes: &mut Vec<Change>) {
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return;
        };
        let Some(zone) = self.zone_of_watch(parent) else {
            return;
        };
        let is_folder = fs::symlink_metadata(path).is_ok_and(|details| details.is_dir());
        if policy::keeps(&zone, name, is_folder) {
            self.add(&zone, name, path.to_owned(), changes);
        }
    }

    fn read_again(&mut self, paths: &[PathBuf], changes: &mut Vec<Change>) {
        for path in paths {
            self.forget(path);
            changes.push(Change::Removed { path: path.clone() });
        }
        for path in paths {
            self.add_again(path, changes);
        }
    }

    fn zone_of_watch(&self, path: &Path) -> Option<Zone> {
        let wd = self.by_path.get(path)?;
        self.by_wd.get(wd).map(|watched| watched.zone.clone())
    }

    fn add(&mut self, zone: &Zone, name: &OsStr, path: PathBuf, changes: &mut Vec<Change>) {
        let Ok(details) = fs::symlink_metadata(&path) else {
            self.vanished.push(path);
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

    fn add_folder(
        &mut self,
        path: PathBuf,
        zone: &Zone,
        details: &fs::Metadata,
        changes: &mut Vec<Change>,
    ) -> Vec<(PathBuf, Zone, fs::Metadata)> {
        if SystemMounts::default().is_mount_root(&path) {
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
                changes.push(Change::Added {
                    details: crawl::facts(details, Kind::Folder),
                    path,
                });
                return Vec::new();
            }
        };
        if is_marked(&path, &children) {
            self.forget(&path);
            changes.push(Change::Removed { path });
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

    fn watch(&mut self, path: PathBuf, zone: Zone) -> Option<Change> {
        match inotify::add_watch(&self.inotify, &path, WATCHED) {
            Ok(wd) => {
                // inotify returns the watch a folder already has, such as one moved here before its old events were read.
                // https://man7.org/linux/man-pages/man7/inotify.7.html
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
            Err(Errno::NOENT | Errno::NOTDIR | Errno::ACCESS | Errno::NOSPC) => None,
            Err(_) => Some(Change::Rescan { folder: path }),
        }
    }

    fn forget_if_folder(&mut self, path: &Path, is_folder: bool) {
        if is_folder {
            self.forget(path);
        }
    }

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
                // The kernel removes a watch itself when its folder goes (IN_IGNORED).
                // https://man7.org/linux/man-pages/man7/inotify.7.html
                inotify::remove_watch(&self.inotify, wd).ok();
            }
        }
    }

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

const fn was_read(entry: &crate::Entry) -> bool {
    matches!(entry.kind, Kind::Folder)
        && !entry.flags.contains(Flags::UNREADABLE)
        && !entry.flags.contains(Flags::OTHER_DRIVE)
}

fn is_marked(path: &Path, children: &[crawl::Child]) -> bool {
    let names: Vec<OsString> = children.iter().map(|child| child.name.clone()).collect();
    policy::marked(&names)
        || (names.iter().any(|name| name == policy::CACHE_TAG)
            && crawl::has_cache_signature(&path.join(policy::CACHE_TAG)))
}

#[cfg(test)]
mod tests;
