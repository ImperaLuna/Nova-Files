use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use crate::changes::Change;

#[derive(Debug, Default)]
pub struct Overlay {
    steps: Vec<Step>,
    gone: Gone,
    added: BTreeSet<PathBuf>,
}

#[derive(Debug)]
enum Step {
    Moved { from: PathBuf, to: PathBuf },
    Gone(Gone),
}

#[derive(Debug, Default)]
struct Gone {
    removed: HashSet<PathBuf>,
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

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty() && self.gone.is_empty() && self.added.is_empty()
    }

    pub const fn moves(&self) -> bool {
        !self.steps.is_empty()
    }

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
            self.gone.renamed.insert(to.to_owned());
            self.added.insert(to.to_owned());
        }
    }

    fn added_under(&self, path: &Path) -> Vec<PathBuf> {
        self.added
            .range(path.to_owned()..)
            .take_while(|added| added.starts_with(path))
            .cloned()
            .collect()
    }

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

    pub fn added(&self) -> impl Iterator<Item = &Path> {
        self.added.iter().map(PathBuf::as_path)
    }
}

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
