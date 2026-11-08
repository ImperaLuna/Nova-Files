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
    /// Folders that now say they aren't worth indexing.
    marked: Vec<PathBuf>,
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
        }
    }

    /// Checks the rows from `rows.start` up to `rows.end`, each at its
    /// path under `to` in place of `from`, skipping what lies inside a
    /// folder that is gone.
    fn check_rows(
        &mut self,
        root: &Path,
        rows: std::ops::Range<u32>,
        moved: Option<(&Path, &Path)>,
        changes: &mut Vec<Change>,
    ) {
        let index = self.index;
        let mut row = rows.start;
        while row < rows.end {
            let at = RowId(row);
            row += 1;
            let Some(path) = index.path(at) else {
                continue;
            };
            let path = match moved {
                Some((from, to)) => match path.strip_prefix(from) {
                    Ok(inside) => to.join(inside),
                    Err(_) => continue,
                },
                None => path,
            };
            if self.check(root, at, path, changes) == Inside::Skip
                && let Some(entry) = index.entry(at)
                && entry.kind == Kind::Folder
            {
                row = row.max(entry.end.0);
            }
        }
    }

    /// Reads the folder at `row` again, now at `path`, when its time moved
    /// or may hide a change.
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

impl Round<'_> {
    /// Checks every folder inside `folder`, itself included, at its place
    /// under `to`, where it was moved.
    fn check_moved(&mut self, root: &Path, folder: RowId, to: &Path, changes: &mut Vec<Change>) {
        let index = self.index;
        let (Some(from), Some(entry)) = (index.path(folder), index.entry(folder)) else {
            return;
        };
        self.check_rows(root, folder.0..entry.end.0, Some((&from, to)), changes);
    }
}

impl Folders {
    /// The changes since `index` was crawled, found by reading the folders
    /// whose modified time moved. Folders moved while nothing watched are
    /// then checked at their new place, and so on down.
    pub fn catch_up(&mut self, index: &Index) -> Vec<Change> {
        let mut changes = Vec::new();
        let mut round = Round::new(index);
        round.check_rows(&self.root, 0..index.rows(), None, &mut changes);
        // One round per moved folder: an entry moved between two folders
        // that were themselves moved reads as removed and added.
        let mut moved = self.settle(round, &mut changes);
        while let Some((folder, to)) = moved.pop() {
            let mut round = Round::new(index);
            round.check_moved(&self.root, folder, &to, &mut changes);
            moved.extend(self.settle(round, &mut changes));
        }
        changes
    }

    /// Pairs what was found with what went missing by file id: moves.
    /// The rest are removals and additions. Removals come first, then
    /// moves, then additions, so a name that now holds another file ends
    /// up added. Returns the moved folders, by their old row and new path.
    fn settle(&mut self, round: Round<'_>, changes: &mut Vec<Change>) -> Vec<(RowId, PathBuf)> {
        let Round {
            index,
            found,
            missing,
            marked,
            ..
        } = round;
        for folder in marked {
            self.forget(&folder);
            changes.push(Change::Removed { path: folder });
        }
        // Hard links share an id, so each id keeps a list.
        let mut by_id: HashMap<FileId, Vec<Missing>> = HashMap::new();
        for old in missing {
            by_id.entry(old.id).or_default().push(old);
        }
        let mut moved = Vec::new();
        let mut added = Vec::new();
        for new in found {
            match by_id.get_mut(&new.id).and_then(Vec::pop) {
                Some(old) => moved.push((old, new)),
                None => added.push(new),
            }
        }
        for old in by_id.into_values().flatten() {
            self.forget(&old.path);
            changes.push(Change::Removed { path: old.path });
        }
        // Every old place is left before any new one is watched, since a
        // move can land where another one left.
        for (old, _) in &moved {
            self.forget(&old.path);
        }
        let mut folders = Vec::new();
        let mut read_again = Vec::new();
        for (old, new) in in_replay_order(moved, changes) {
            let Some(old) = old else {
                read_again.push(new);
                continue;
            };
            changes.push(Change::Moved {
                from: old.path.clone(),
                to: new.path.clone(),
            });
            if new.details.is_dir() {
                // Its contents moved with it; only the watches are new.
                let mut unused = Vec::new();
                self.add(&new.zone, &new.name, new.path.clone(), &mut unused);
                folders.push((old.row, new.path));
            } else if let Some(entry) = index.entry(old.row) {
                // Written while closed, perhaps, as well as moved.
                changed(&entry, new.path, &new.details, changes);
            }
        }
        for new in added.into_iter().chain(read_again) {
            self.add(&new.zone, &new.name, new.path, changes);
        }
        folders
    }
}

/// Orders `moves` so that each lands only after whatever stood at its new
/// path left, as replaying them in the log needs. A ring of moves (two
/// names swapped) has no such order: one of them is logged as removed
/// instead and comes back without its old half, to be added whole.
fn in_replay_order(
    moves: Vec<(Missing, Found)>,
    changes: &mut Vec<Change>,
) -> Vec<(Option<Missing>, Found)> {
    let mut waiting: Vec<(Missing, Found)> = moves;
    let mut ordered = Vec::with_capacity(waiting.len());
    while !waiting.is_empty() {
        let leaving: HashSet<&Path> = waiting.iter().map(|(old, _)| old.path.as_path()).collect();
        let ready = waiting
            .iter()
            .position(|(_, new)| !leaving.contains(new.path.as_path()));
        let (old, new) = waiting.swap_remove(ready.unwrap_or(0));
        if ready.is_some() {
            ordered.push((Some(old), new));
        } else {
            changes.push(Change::Removed { path: old.path });
            ordered.push((None, new));
        }
    }
    ordered
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
                            details,
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
