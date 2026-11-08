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
//! A file moved while nobody watched is logged as removed at its old
//! place and added at its new one; the addition carries its device and
//! inode, so a reader can still tell it is the same file. Removals come
//! before additions, so replaying them in order keeps both halves of a
//! swap. A folder moved, or replaced by another, while nobody watched
//! makes the whole catch-up one [`Change::Rescan`] of home: its rows say
//! nothing of where its contents are now, or of which rules hold for them
//! at the new place, and a fresh crawl answers both. Files inside folders
//! whose time didn't change can lag in size and date until they are
//! written again: names never do.
//!
//! A folder changed within [`SAME_TICK`] before the crawl started is read
//! again even when its time looks the same: file times come from a coarse
//! clock, and a change right after the crawl read it can keep its time.
//!
//! It runs after every watch is installed, so whatever changes while it
//! reads is also reported by the watches, at worst twice.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};

use crate::changes::Change;
use crate::crawl;
use crate::policy::{self, Zone};
use crate::reader::Index;
use crate::{Entry, FileId, Kind, RowId, Stamp};

use super::{Folders, is_marked, was_read};

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
    details: Metadata,
}

/// An entry in the index and not on disk.
struct Missing {
    path: PathBuf,
    id: FileId,
}

/// One round of reading folders again: what turned up and what went.
struct Round<'a> {
    index: &'a Index,
    /// Folders whose stored time is at least this may hide a change.
    unsure: Option<Stamp>,
    found: Vec<Found>,
    missing: Vec<Missing>,
    /// Folders that now say they aren't worth indexing.
    marked: Vec<PathBuf>,
    /// A folder now holds another folder than its row: only a crawl can
    /// tell what is inside.
    replaced: bool,
}

/// What checking a folder row says about the rows inside it.
#[derive(Debug, PartialEq, Eq)]
enum Inside {
    /// Check them too.
    Check,
    /// Skip them: the folder is gone, is no folder any more, or is left
    /// out now. Its parent's read says what became of it; reading paths
    /// under it could follow a link that took its place.
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

    /// Checks every row, skipping what lies inside a folder that is gone,
    /// and stopping once a folder was replaced.
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
            // Only a folder row is ever skipped.
            if self.check(root, at, path, changes) == Inside::Skip
                && let Some(entry) = index.entry(at)
            {
                rows.start = rows.start.max(entry.end.0);
            }
        }
    }

    /// Reads the folder at `row`, at `path`, again when its time moved or
    /// may hide a change.
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
            // Home itself is indexed whatever it holds.
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
    /// The changes since `index` was crawled, found by reading the folders
    /// whose modified time moved; one [`Change::Rescan`] of home when a
    /// folder moved or was replaced.
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

    /// Logs what went missing as removed, then what was found as added,
    /// so a name that now holds another file ends up added. `false`, with
    /// nothing logged, when a folder was replaced or moved: a folder found
    /// with the id of one that went missing.
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
        for folder in marked {
            self.forget(&folder);
            changes.push(Change::Removed { path: folder });
        }
        for old in missing {
            self.forget(&old.path);
            changes.push(Change::Removed { path: old.path });
        }
        for new in found {
            self.add(&new.zone, &new.name, new.path, changes);
        }
        true
    }
}

/// One folder whose modified time moved.
struct Compared<'a> {
    index: &'a Index,
    row: RowId,
    may_be_marked: bool,
    path: PathBuf,
    zone: Zone,
}

impl Compared<'_> {
    /// Compares the folder with its rows; [`Inside::Skip`] when it holds
    /// a marker and is left out now.
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
                // Listed, so still there; its details can't be read now
                // (no search permission), so its row stays as it is.
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
