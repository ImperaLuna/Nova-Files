use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::changes::Change;

#[derive(Debug, Default)]
pub struct Overlay {
    steps: Vec<Step>,
    gone: Gone,
    added: BTreeSet<PathBuf>,
    moved_rows: OnceLock<Vec<Range<u32>>>,
    moves_from: Steps,
    removed_at: Steps,
    renamed_at: Steps,
    touched: HashSet<PathBuf>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Held {
    Itself,
    Inside,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Whole {
    At(PathBuf),
    Gone,
    Mixed,
}

type Steps = HashMap<PathBuf, Vec<usize>>;

fn next_at(steps: &Steps, path: &Path, from: usize) -> Option<usize> {
    let at = steps.get(path)?;
    at.get(at.partition_point(|&step| step < from)).copied()
}

#[derive(Debug)]
enum Step {
    Moved { from: PathBuf, to: PathBuf },
    Gone,
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
        self.renamed.contains(path) || self.holds_under(path)
    }

    fn holds_under(&self, path: &Path) -> bool {
        path.ancestors().any(|folder| self.removed.contains(folder))
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
                self.touch(path);
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
            let (gone, at) = (std::mem::take(&mut self.gone), self.steps.len());
            for path in &gone.removed {
                self.removed_at.entry(path.clone()).or_default().push(at);
            }
            for path in &gone.renamed {
                self.renamed_at.entry(path.clone()).or_default().push(at);
            }
            self.steps.push(Step::Gone);
        }
        self.touch(from);
        self.touch(to);
        self.moves_from
            .entry(from.to_owned())
            .or_default()
            .push(self.steps.len());
        self.steps.push(Step::Moved {
            from: from.to_owned(),
            to: to.to_owned(),
        });
        self.moved_rows = OnceLock::new();
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

    fn touch(&mut self, path: &Path) {
        if let Some(folder) = path.parent() {
            self.touched.insert(folder.to_owned());
        }
    }

    pub fn place(&self, path: PathBuf) -> Option<PathBuf> {
        self.follow(path, Held::Itself, |_| {})
    }

    pub fn place_whole(&self, folder: PathBuf) -> Whole {
        let mut mixed = false;
        let placed = self.follow(folder, Held::Inside, |at| {
            mixed |= self.touched.contains(at);
        });
        match placed {
            _ if mixed => Whole::Mixed,
            Some(folder) => Whole::At(folder),
            None => Whole::Gone,
        }
    }

    fn follow(&self, path: PathBuf, held: Held, mut passing: impl FnMut(&Path)) -> Option<PathBuf> {
        let mut path = path;
        let mut done = 0;
        loop {
            passing(&path);
            let next = path
                .ancestors()
                .filter_map(|above| next_at(&self.moves_from, above, done))
                .min();
            let until = next.unwrap_or(self.steps.len());
            if self.gone_between(&path, done..until, held) {
                return None;
            }
            let Some(Step::Moved { from, to }) = next.and_then(|at| self.steps.get(at)) else {
                break;
            };
            path = rebase(&path, from, to)?;
            done = until + 1;
        }
        let gone = match held {
            Held::Itself => self.gone.holds(&path),
            Held::Inside => self.gone.holds_under(&path),
        };
        (!gone).then_some(path)
    }

    fn gone_between(&self, path: &Path, steps: Range<usize>, held: Held) -> bool {
        let within = |at: Option<usize>| at.is_some_and(|at| at < steps.end);
        held == Held::Itself && within(next_at(&self.renamed_at, path, steps.start))
            || path
                .ancestors()
                .any(|above| within(next_at(&self.removed_at, above, steps.start)))
    }

    pub fn moved_rows(&self, find: impl FnOnce() -> Vec<Range<u32>>) -> &[Range<u32>] {
        self.moved_rows.get_or_init(find)
    }

    pub fn moved_from(&self) -> impl Iterator<Item = &Path> {
        self.steps.iter().filter_map(|step| match step {
            Step::Moved { from, .. } => Some(from.as_path()),
            Step::Gone => None,
        })
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
