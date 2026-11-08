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

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::changes::Change;

/// The changes since the crawl, as steps for indexed paths and entries to
/// add.
#[derive(Debug, Default)]
pub struct Overlay {
    steps: Vec<Step>,
    /// Entries the index doesn't hold under their current path and name.
    added: BTreeSet<PathBuf>,
}

/// What happened to indexed paths, in order.
#[derive(Debug)]
enum Step {
    /// Everything at or under `from` is now under `to`.
    Moved { from: PathBuf, to: PathBuf },
    /// Everything at or under the path is gone.
    Removed(PathBuf),
    /// Only this path is gone from the index's view: its row's name is out
    /// of date, and it is kept under `added` instead.
    Renamed(PathBuf),
}

impl Overlay {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether nothing changed since the crawl.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty() && self.added.is_empty()
    }

    /// Takes in one change, in log order.
    pub fn apply(&mut self, change: &Change) {
        match change {
            Change::Added { path, .. } => {
                self.added.insert(path.clone());
            }
            Change::Removed { path } => {
                self.added.retain(|added| !added.starts_with(path));
                self.steps.push(Step::Removed(path.clone()));
            }
            Change::Moved { from, to } => self.moved(from, to),
            // Names and places stay; a rescan is its owner's to handle.
            Change::Changed { .. } | Change::Rescan { .. } => {}
        }
    }

    fn moved(&mut self, from: &Path, to: &Path) {
        let moved: Vec<PathBuf> = self
            .added
            .extract_if(.., |added| added.starts_with(from))
            .filter_map(|added| rebase(&added, from, to))
            .collect();
        self.added.extend(moved);
        self.steps.push(Step::Moved {
            from: from.to_owned(),
            to: to.to_owned(),
        });
        if from.file_name() != to.file_name() {
            // The moved entry's own row still has its old name.
            self.steps.push(Step::Renamed(to.to_owned()));
            self.added.insert(to.to_owned());
        }
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
                Step::Removed(gone) if path.starts_with(gone) => return None,
                Step::Renamed(renamed) if path == *renamed => return None,
                Step::Removed(_) | Step::Renamed(_) => {}
            }
        }
        Some(path)
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
