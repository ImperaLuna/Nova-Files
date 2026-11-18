use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};

use rustix::fs::inotify;

use crate::changes::Change;
use crate::crawl;
use crate::policy::{self, Zone};
use crate::reader::Index;
use crate::{Entry, FileId, Kind, RowId, Stamp};

use super::{Folders, is_marked, was_read};

const SAME_TICK: i64 = 2_000_000_000;

struct Found {
    path: PathBuf,
    name: OsString,
    zone: Zone,
    id: FileId,
    details: Metadata,
}

struct Missing {
    path: PathBuf,
    id: FileId,
}

struct Round<'a> {
    index: &'a Index,
    unsure: Option<Stamp>,
    found: Vec<Found>,
    missing: Vec<Missing>,
    marked: Vec<PathBuf>,
    replaced: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum Inside {
    Check,
    Skip,
}

impl<'a> Round<'a> {
    fn new(index: &'a Index) -> Self {
        Self {
            index,
            unsure: index
                .crawled()
                .map(|Stamp(nanos)| Stamp(nanos.saturating_sub(SAME_TICK))),
            found: Vec::new(),
            missing: Vec::new(),
            marked: Vec::new(),
            replaced: false,
        }
    }

    fn check_rows(&mut self, root: &Path, changes: &mut Vec<Change>) {
        let index = self.index;
        let mut rows = 0..index.rows();
        while let Some(row) = rows.next() {
            if self.replaced {
                return;
            }
            let at = RowId(row);
            let Some(path) = index.path(at) else {
                continue;
            };
            if self.check(root, at, path, changes) == Inside::Skip
                && let Some(entry) = index.entry(at)
            {
                rows.start = rows.start.max(entry.end.0);
            }
        }
    }

    fn check(
        &mut self,
        root: &Path,
        row: RowId,
        path: PathBuf,
        changes: &mut Vec<Change>,
    ) -> Inside {
        let Some(entry) = self.index.entry(row).filter(was_read) else {
            return Inside::Check;
        };
        let zone = path.strip_prefix(root).ok().and_then(policy::zone_of);
        let Some(zone) = zone else {
            return Inside::Skip;
        };
        let Ok(details) = fs::symlink_metadata(&path) else {
            return Inside::Skip;
        };
        if !details.is_dir() {
            return Inside::Skip;
        }
        let now = crawl::facts(&details, Kind::Folder);
        if now.id != entry.id {
            self.replaced = true;
            return Inside::Skip;
        }
        if now.modified == entry.modified {
            let recent =
                matches!((entry.modified, self.unsure), (Some(was), Some(unsure)) if was >= unsure);
            if !recent {
                return Inside::Check;
            }
        } else {
            changes.push(Change::Changed {
                details: now,
                path: path.clone(),
            });
        }
        let folder = Compared {
            index: self.index,
            row,
            may_be_marked: path != root,
            path,
            zone,
        };
        let inside = folder.compare(&mut self.found, &mut self.missing, changes);
        if inside == Inside::Skip {
            self.marked.push(folder.path);
        }
        inside
    }
}

impl Folders {
    pub fn catch_up(&mut self, index: &Index) -> Vec<Change> {
        let mut changes = Vec::new();
        let mut round = Round::new(index);
        round.check_rows(&self.root, &mut changes);
        if !self.settle(round, &mut changes) {
            return vec![Change::Rescan {
                folder: self.root.clone(),
            }];
        }
        changes
    }

    fn settle(&mut self, round: Round<'_>, changes: &mut Vec<Change>) -> bool {
        let Round {
            found,
            missing,
            marked,
            replaced,
            ..
        } = round;
        let gone: HashSet<FileId> = missing.iter().map(|old| old.id).collect();
        let moved_folder = found
            .iter()
            .any(|new| new.details.is_dir() && gone.contains(&new.id));
        if replaced || moved_folder {
            return false;
        }
        let mut unwatched: HashSet<PathBuf> = marked.iter().cloned().collect();
        unwatched.extend(missing.iter().map(|old| old.path.clone()));
        self.forget_all(&unwatched);
        for folder in marked {
            changes.push(Change::Removed { path: folder });
        }
        for old in missing {
            changes.push(Change::Removed { path: old.path });
        }
        for new in found {
            self.add(&new.zone, &new.name, new.path, changes);
        }
        true
    }
}

impl Folders {
    fn forget_all(&mut self, folders: &HashSet<PathBuf>) {
        if folders.is_empty() {
            return;
        }
        let under: Vec<PathBuf> = self
            .by_path
            .keys()
            .filter(|watched| watched.ancestors().any(|folder| folders.contains(folder)))
            .cloned()
            .collect();
        for watched in under {
            if let Some(wd) = self.by_path.remove(&watched) {
                self.by_wd.remove(&wd);
                inotify::remove_watch(&self.inotify, wd).ok();
            }
        }
    }
}

struct Compared<'a> {
    index: &'a Index,
    row: RowId,
    may_be_marked: bool,
    path: PathBuf,
    zone: Zone,
}

impl Compared<'_> {
    fn compare(
        &self,
        found: &mut Vec<Found>,
        missing: &mut Vec<Missing>,
        changes: &mut Vec<Change>,
    ) -> Inside {
        let Ok(children) = crawl::read_children(&self.path, &|| false) else {
            return Inside::Check;
        };
        if self.may_be_marked && is_marked(&self.path, &children) {
            return Inside::Skip;
        }
        let mut indexed: HashMap<OsString, (RowId, Entry)> = self
            .index
            .children(self.row)
            .filter_map(|row| {
                Some((
                    self.index.name(row)?.to_owned(),
                    (row, self.index.entry(row)?),
                ))
            })
            .collect();
        for child in children {
            let old = indexed.remove(&child.name);
            let Ok(details) = child.details else {
                continue;
            };
            let path = self.path.join(&child.name);
            let id = FileId {
                device: std::os::unix::fs::MetadataExt::dev(&details),
                inode: child.inode,
            };
            match old {
                Some((_, old)) if old.id == id => changed(&old, path, &details, changes),
                _ => {
                    if let Some((_, old)) = old {
                        missing.push(Missing {
                            path: path.clone(),
                            id: old.id,
                        });
                    }
                    if policy::keeps(&self.zone, &child.name, details.is_dir()) {
                        found.push(Found {
                            path,
                            name: child.name,
                            zone: self.zone.clone(),
                            id,
                            details,
                        });
                    }
                }
            }
        }
        for (name, (_, old)) in indexed {
            missing.push(Missing {
                path: self.path.join(name),
                id: old.id,
            });
        }
        Inside::Check
    }
}

fn changed(old: &Entry, path: PathBuf, details: &Metadata, changes: &mut Vec<Change>) {
    if old.kind == Kind::Folder {
        return;
    }
    let kind = if details.is_symlink() {
        Kind::Link
    } else {
        Kind::File
    };
    let now = crawl::facts(details, kind);
    if now.size != old.size || now.modified != old.modified {
        changes.push(Change::Changed { path, details: now });
    }
}
