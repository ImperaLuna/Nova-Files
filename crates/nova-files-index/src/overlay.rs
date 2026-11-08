//! What the change log says happened since the crawl, kept so that a
//! search over the index sees the disk as it is now.
//!
//! The index itself is never edited. Each [`Change`] is applied to an
//! [`Overlay`] instead: removals and moves become steps that every indexed
//! path is put through, in order, and entries the index doesn't have yet
//! are kept by path. Putting a path through the steps is cheap, and a
//! search only does it for its best hits, so the overlay can grow until
//! the core writes a fresh index (and starts an empty overlay).
//!
//! A [`Change::Rescan`] means changes were lost: the overlay can't tell
//! what they were, so its owner reads the folder again or crawls anew.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use crate::changes::Change;

/// The changes since the crawl, as steps for indexed paths and entries to
/// add.
#[derive(Debug, Default)]
pub struct Overlay {
    steps: Vec<Step>,
    /// What went since the last move, not yet a step.
    gone: Gone,
    /// Entries the index doesn't hold under their current path and name.
    added: BTreeSet<PathBuf>,
}

/// What happened to indexed paths, in order.
#[derive(Debug)]
enum Step {
    /// Everything at or under `from` is now under `to`.
    Moved {
        from: PathBuf,
        to: PathBuf,
    },
    Gone(Gone),
}

/// Paths gone between two moves. Their order among themselves doesn't
/// matter, so they are kept in sets: a path is looked up once per folder
/// it is in, however many went.
#[derive(Debug, Default)]
struct Gone {
    /// Everything at or under each of these is gone.
    removed: HashSet<PathBuf>,
    /// Only these paths are gone from the index's view: their rows' names
    /// are out of date, and they are kept under `added` instead.
    renamed: HashSet<PathBuf>,
}

impl Gone {
    fn is_empty(&self) -> bool {
        self.removed.is_empty() && self.renamed.is_empty()
    }

    fn holds(&self, path: &Path) -> bool {
        self.renamed.contains(path) || path.ancestors().any(|folder| self.removed.contains(folder))
    }
}

impl Overlay {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether nothing changed since the crawl.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty() && self.gone.is_empty() && self.added.is_empty()
    }

    /// Whether some indexed paths may be elsewhere now.
    pub const fn moves(&self) -> bool {
        !self.steps.is_empty()
    }

    /// Takes in one change, in log order.
    pub fn apply(&mut self, change: &Change) {
        match change {
            Change::Added { path, .. } => {
                self.added.insert(path.clone());
            }
            Change::Removed { path } => {
                for gone in self.added_under(path) {
                    self.added.remove(&gone);
                }
                self.gone.removed.insert(path.clone());
            }
            Change::Moved { from, to } => self.moved(from, to),
            // Names and places stay; a rescan is its owner's to handle.
            Change::Changed { .. } | Change::Rescan { .. } => {}
        }
    }

    fn moved(&mut self, from: &Path, to: &Path) {
        let under = self.added_under(from);
        for added in &under {
            self.added.remove(added);
        }
        self.added
            .extend(under.iter().filter_map(|added| rebase(added, from, to)));
        if !self.gone.is_empty() {
            self.steps.push(Step::Gone(std::mem::take(&mut self.gone)));
        }
        self.steps.push(Step::Moved {
            from: from.to_owned(),
            to: to.to_owned(),
        });
        if from.file_name() != to.file_name() {
            // The moved entry's own row still has its old name.
            self.gone.renamed.insert(to.to_owned());
            self.added.insert(to.to_owned());
        }
    }

    /// The added entries at or under `path`. Paths sort by their parts,
    /// so they sit together from `path` on.
    fn added_under(&self, path: &Path) -> Vec<PathBuf> {
        self.added
            .range(path.to_owned()..)
            .take_while(|added| added.starts_with(path))
            .cloned()
            .collect()
    }

    /// Where the entry indexed at `path` is now, or `None` when it is gone
    /// or is kept under its new name among the added ones.
    pub fn place(&self, path: PathBuf) -> Option<PathBuf> {
        let mut path = path;
        for step in &self.steps {
            match step {
                Step::Moved { from, to } => {
                    if let Some(moved) = rebase(&path, from, to) {
                        path = moved;
                    }
                }
                Step::Gone(gone) if gone.holds(&path) => return None,
                Step::Gone(_) => {}
            }
        }
        (!self.gone.holds(&path)).then_some(path)
    }

    /// The entries the index doesn't hold, by their current path.
    pub fn added(&self) -> impl Iterator<Item = &Path> {
        self.added.iter().map(PathBuf::as_path)
    }
}

/// `path` with `from` swapped for `to`, when it is at or under `from`.
fn rebase(path: &Path, from: &Path, to: &Path) -> Option<PathBuf> {
    let rest = path.strip_prefix(from).ok()?;
    Some(if rest.as_os_str().is_empty() {
        to.to_owned()
    } else {
        to.join(rest)
    })
}

#[cfg(test)]
mod tests;
