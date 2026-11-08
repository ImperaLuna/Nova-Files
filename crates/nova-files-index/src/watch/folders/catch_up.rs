//! Catching up with what changed while nothing watched: before the watches
//! were installed, including while Nova Files wasn't running.
//!
//! A folder's modified time changes whenever an entry is added to it,
//! removed from it or renamed in it, so only folders whose time differs
//! from the index are read again; the rest cost one `stat` each. A read
//! folder is compared with its rows by name: new names are added, missing
//! ones removed, and a file whose size or modified time differs is changed.
//! A name now holding another file (another inode) counts as one removed
//! and one added; a file deleted and made again under the same name can
//! get its old inode back, and then reads as changed.
//!
//! An entry removed in one place and added in another with the same
//! device and inode was moved while nobody watched: one [`Change::Moved`],
//! the moved folder's watches are installed at its new place, and its
//! folders are checked there in another round. Files
//! inside folders whose time didn't change can lag in size and date until
//! they are written again: names never do.
//!
//! A folder changed within [`SAME_TICK`] before the crawl started is read
//! again even when its time looks the same: file times come from a coarse
//! clock, and a change right after the crawl read it can keep its time.
//!
//! It runs after every watch is installed, so whatever changes while it
//! reads is also reported by the watches, at worst twice.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};

use crate::changes::Change;
use crate::crawl;
use crate::policy::{self, Zone};
use crate::reader::Index;
use crate::{Entry, FileId, Kind, RowId, Stamp};

use super::{Folders, was_read};

/// How close to the crawl's start a folder's time can be and still hide a
/// later change. Filesystem clocks tick every few milliseconds; some
/// (FAT, network shares) only every one or two seconds.
const SAME_TICK: i64 = 2_000_000_000;

/// An entry found on disk and not in the index.
struct Found {
    path: PathBuf,
    name: OsString,
    zone: Zone,
    id: FileId,
    is_folder: bool,
}

/// An entry in the index and not on disk.
struct Missing {
    path: PathBuf,
    row: RowId,
    id: FileId,
}

/// One round of reading folders again: what turned up and what went.
struct Round<'a> {
    index: &'a Index,
    /// Folders whose stored time is at least this may hide a change.
    unsure: Option<Stamp>,
    found: Vec<Found>,
    missing: Vec<Missing>,
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
        }
    }

    /// Reads the folder at `row` again, now at `path`, when its time moved
    /// or may hide a change.
    fn check(&mut self, root: &Path, row: RowId, path: PathBuf, changes: &mut Vec<Change>) {
        let Some(entry) = self.index.entry(row).filter(was_read) else {
            return;
        };
        let zone = path.strip_prefix(root).ok().and_then(policy::zone_of);
        let Some(zone) = zone else {
            return;
        };
        let Ok(details) = fs::symlink_metadata(&path) else {
            // Gone: its parent's read says so.
            return;
        };
        if !details.is_dir() {
            return;
        }
        let now = crawl::facts(&details, Kind::Folder);
        if now.modified == entry.modified {
            let recent =
                matches!((entry.modified, self.unsure), (Some(was), Some(unsure)) if was >= unsure);
            if !recent {
                return;
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
            path,
            zone,
        };
        folder.compare(&mut self.found, &mut self.missing, changes);
    }
}

impl Round<'_> {
    /// Checks every folder inside `folder`, itself included, at its place
    /// under `to`, where it was moved.
    fn check_moved(&mut self, root: &Path, folder: RowId, to: &Path, changes: &mut Vec<Change>) {
        let index = self.index;
        let (Some(from), Some(entry)) = (index.path(folder), index.entry(folder)) else {
            return;
        };
        for row in (folder.0..entry.end.0).map(RowId) {
            let path = index.path(row);
            if let Some(inside) = path
                .as_deref()
                .and_then(|path| path.strip_prefix(&from).ok())
            {
                self.check(root, row, to.join(inside), changes);
            }
        }
    }
}

impl Folders {
    /// The changes since `index` was crawled, found by reading the folders
    /// whose modified time moved. Folders moved while nothing watched are
    /// then checked at their new place, and so on down.
    pub fn catch_up(&mut self, index: &Index) -> Vec<Change> {
        let mut changes = Vec::new();
        let mut round = Round::new(index);
        for row in (0..index.rows()).map(RowId) {
            if let Some(path) = index.path(row) {
                round.check(&self.root, row, path, &mut changes);
            }
        }
        // One round per moved folder: an entry moved between two folders
        // that were themselves moved reads as removed and added.
        let mut moved = self.settle(round.found, round.missing, &mut changes);
        while let Some((folder, to)) = moved.pop() {
            let mut round = Round::new(index);
            round.check_moved(&self.root, folder, &to, &mut changes);
            moved.extend(self.settle(round.found, round.missing, &mut changes));
        }
        changes
    }

    /// Pairs what was found with what went missing by file id: moves.
    /// The rest are removals and additions. Removals come first, then
    /// moves, then additions, so a name that now holds another file ends
    /// up added. Returns the moved folders, by their old row and new path.
    fn settle(
        &mut self,
        found: Vec<Found>,
        mut missing: Vec<Missing>,
        changes: &mut Vec<Change>,
    ) -> Vec<(RowId, PathBuf)> {
        let mut moved = Vec::new();
        let mut added = Vec::new();
        for new in found {
            match missing.iter().position(|old| old.id == new.id) {
                Some(at) => moved.push((missing.swap_remove(at), new)),
                None => added.push(new),
            }
        }
        for old in missing {
            self.forget(&old.path);
            changes.push(Change::Removed { path: old.path });
        }
        let mut folders = Vec::new();
        for (old, new) in moved {
            self.forget(&old.path);
            changes.push(Change::Moved {
                from: old.path,
                to: new.path.clone(),
            });
            if new.is_folder {
                // Its contents moved with it; only the watches are new.
                let mut unused = Vec::new();
                self.add(&new.zone, &new.name, new.path.clone(), &mut unused);
                folders.push((old.row, new.path));
            }
        }
        for new in added {
            self.add(&new.zone, &new.name, new.path, changes);
        }
        folders
    }
}

/// One folder whose modified time moved.
struct Compared<'a> {
    index: &'a Index,
    row: RowId,
    path: PathBuf,
    zone: Zone,
}

impl Compared<'_> {
    fn compare(
        &self,
        found: &mut Vec<Found>,
        missing: &mut Vec<Missing>,
        changes: &mut Vec<Change>,
    ) {
        let Ok(children) = crawl::read_children(&self.path, &|| false) else {
            return;
        };
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
            let Ok(details) = child.details else {
                continue;
            };
            let path = self.path.join(&child.name);
            let id = FileId {
                device: std::os::unix::fs::MetadataExt::dev(&details),
                inode: child.inode,
            };
            let old = indexed.remove(&child.name);
            match old {
                Some((_, old)) if old.id == id => changed(&old, path, &details, changes),
                _ => {
                    if let Some((row, old)) = old {
                        missing.push(Missing {
                            path: path.clone(),
                            row,
                            id: old.id,
                        });
                    }
                    if policy::keeps(&self.zone, &child.name, details.is_dir()) {
                        found.push(Found {
                            path,
                            name: child.name,
                            zone: self.zone.clone(),
                            id,
                            is_folder: details.is_dir(),
                        });
                    }
                }
            }
        }
        for (name, (row, old)) in indexed {
            missing.push(Missing {
                path: self.path.join(name),
                row,
                id: old.id,
            });
        }
    }
}

/// A file still in place: changed when its size or modified time moved.
/// A folder is checked on its own, as a row of the index.
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
