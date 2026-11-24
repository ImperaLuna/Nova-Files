mod matching;
mod terms;

use std::collections::HashSet;
use std::ffi::OsStr;
use std::num::NonZero;
use std::ops::Range;
use std::panic::resume_unwind;
use std::path::{Path, PathBuf};
use std::thread;

use crate::fold;
use crate::overlay::Overlay;
use crate::reader::Index;
use crate::{Kind, RowId};

pub use matching::MatchKind;
use terms::Terms;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub path: PathBuf,
    pub kind: MatchKind,
}

pub struct Search<'a> {
    index: &'a Index,
    overlay: Option<&'a Overlay>,
    added: Vec<&'a Path>,
    terms: Terms,
    hidden_last: bool,
    limit: usize,
    shown: HashSet<PathBuf>,
    within: Option<PathBuf>,
    rows: Range<u32>,
    stage: Stage,
}

pub const STAGES: usize = 2;
const SCAN_THREADS_MAX: usize = 16;
const ROWS_PER_THREAD_MIN: usize = 32_768;
const KEEP_SLACK: usize = 4;
const KEEP_MIN: usize = 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    AsTyped,
    Loose,
    Done,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Source {
    Row(RowId),
    Added(usize),
}

type Rank = (usize, MatchKind, bool, usize, usize, usize, Source);

struct Scan<'a> {
    index: &'a Index,
    terms: &'a Terms,
    loose: bool,
    all_hidden: bool,
    hidden_last: bool,
    keep: Option<usize>,
}

struct Open {
    ends: Vec<RowId>,
    reach: Vec<u8>,
    terms: usize,
}

impl Open {
    fn above(scan: &Scan<'_>, first: RowId) -> Self {
        let index = scan.index;
        let mut open = Self {
            ends: Vec::new(),
            reach: vec![0; scan.terms.len()],
            terms: scan.terms.len(),
        };
        let mut chain: Vec<RowId> =
            std::iter::successors(index.parent(first), |&folder| index.parent(folder)).collect();
        chain.pop();
        for folder in chain.into_iter().rev() {
            open.enter(scan, folder);
        }
        open
    }

    fn enter(&mut self, scan: &Scan<'_>, folder: RowId) {
        let (Some(end), Some(name)) = (scan.index.end(folder), scan.index.folded(folder)) else {
            return;
        };
        let from = self.reach.len() - self.terms;
        self.reach.extend_from_within(from..);
        scan.terms.enter(&mut self.reach[from + self.terms..], name);
        self.ends.push(end);
    }

    fn leave_before(&mut self, row: RowId) {
        while self.ends.last().is_some_and(|&end| end <= row) {
            self.ends.pop();
            self.reach.truncate(self.reach.len() - self.terms);
        }
    }

    fn reach(&self) -> &[u8] {
        &self.reach[self.reach.len() - self.terms..]
    }
}

impl Scan<'_> {
    fn ranked(&self, rows: Range<u32>) -> Vec<Rank> {
        let index = self.index;
        let mut ranked = Vec::new();
        let mut open = Open::above(self, RowId(rows.start));
        for row in rows.map(RowId) {
            open.leave_before(row);
            if let Some(rank) = self.rank(row, &open) {
                ranked.push(rank);
            }
            if let Some(keep) = self.keep
                && ranked.len() >= keep.saturating_mul(KEEP_SLACK).max(KEEP_MIN)
            {
                ranked.select_nth_unstable(keep);
                ranked.truncate(keep);
            }
            if index.kind(row) == Some(Kind::Folder) {
                open.enter(self, row);
            }
        }
        ranked
    }

    fn rank(&self, row: RowId, open: &Open) -> Option<Rank> {
        let index = self.index;
        let reach = open.reach();
        if !self
            .terms
            .could_meet(reach, index.letters(row)?, self.loose)
        {
            return None;
        }
        let folded = index.folded(row)?;
        let starts_word = |at| index.starts_word(row, at);
        let met = self.terms.meet(reach, folded, self.loose, starts_word)?;
        if self.loose && !met.loosely {
            return None;
        }
        let hidden = self.all_hidden || index.is_hidden(row);
        Some((
            met.by_folders,
            met.found.kind,
            hidden && self.hidden_last,
            met.found.cost,
            folded.len(),
            open.ends.len(),
            Source::Row(row),
        ))
    }
}

impl Index {
    pub fn search(&self, query: &str, limit: usize) -> Search<'_> {
        self.searching(query, limit, None)
    }

    pub fn search_now<'a>(&'a self, query: &str, limit: usize, overlay: &'a Overlay) -> Search<'a> {
        self.searching(query, limit, Some(overlay))
    }

    fn searching<'a>(
        &'a self,
        query: &str,
        limit: usize,
        overlay: Option<&'a Overlay>,
    ) -> Search<'a> {
        let terms = Terms::parse(query);
        let stage = if terms.is_empty() || limit == 0 {
            Stage::Done
        } else {
            Stage::AsTyped
        };
        Search {
            index: self,
            overlay,
            added: overlay
                .map(|overlay| overlay.added().collect())
                .unwrap_or_default(),
            terms,
            hidden_last: !query.trim_start().starts_with('.'),
            limit,
            shown: HashSet::new(),
            within: None,
            rows: 1..self.rows(),
            stage,
        }
    }
}

impl Iterator for Search<'_> {
    type Item = Vec<Hit>;

    fn next(&mut self) -> Option<Vec<Hit>> {
        let stage = self.stage;
        self.stage = match stage {
            Stage::AsTyped => Stage::Loose,
            Stage::Loose | Stage::Done => Stage::Done,
        };
        match stage {
            Stage::AsTyped => Some(self.stage_hits(false)),
            Stage::Loose => Some(self.stage_hits(true)),
            Stage::Done => None,
        }
    }
}

impl Search<'_> {
    #[must_use]
    pub fn within(mut self, folder: &Path) -> Self {
        let index = self.index;
        let holds_root = !folder.starts_with(index.root()) && index.root().starts_with(folder);
        if !holds_root && !self.overlay.is_some_and(Overlay::moves) {
            self.rows = index
                .row_of(folder)
                .and_then(|row| Some(row.0.saturating_add(1)..index.end(row)?.0))
                .unwrap_or(0..0);
        }
        self.within = Some(folder.to_owned());
        self
    }

    pub fn is_done(&self) -> bool {
        self.stage == Stage::Done
    }

    fn stage_hits(&mut self, loose: bool) -> Vec<Hit> {
        let mut ranked = self.scan_rows(loose);
        ranked.extend(self.scan_added(loose));
        self.best(ranked)
    }

    fn scan_rows(&self, loose: bool) -> Vec<Rank> {
        let index = self.index;
        let changed = self.overlay.is_some_and(|overlay| !overlay.is_empty());
        let scan = Scan {
            index,
            terms: &self.terms,
            loose,
            all_hidden: self
                .within
                .as_deref()
                .is_some_and(|folder| is_hidden(index.root(), folder)),
            hidden_last: self.hidden_last,
            keep: (!changed).then_some(self.limit),
        };
        let rows = self.rows.clone();
        let count = usize::try_from(rows.end.saturating_sub(rows.start)).unwrap_or_default();
        let threads = thread::available_parallelism()
            .map_or(1, NonZero::get)
            .min(SCAN_THREADS_MAX)
            .min(count / ROWS_PER_THREAD_MIN)
            .max(1);
        let share = u32::try_from(count.div_ceil(threads)).unwrap_or(u32::MAX);
        let parts = (0..threads).scan(rows.start, |start, _| {
            let end = start.saturating_add(share).min(rows.end);
            Some(std::mem::replace(start, end)..end)
        });
        thread::scope(|scope| {
            #[expect(
                clippy::needless_collect,
                reason = "every part starts before the first is waited for"
            )]
            let running: Vec<_> = parts
                .map(|part| {
                    let scan = &scan;
                    scope.spawn(move || scan.ranked(part))
                })
                .collect();
            running
                .into_iter()
                .flat_map(|part| part.join().unwrap_or_else(|panic| resume_unwind(panic)))
                .collect()
        })
    }

    fn scan_added(&self, loose: bool) -> Vec<Rank> {
        let root = self.index.root();
        let mut ranked = Vec::new();
        let mut folded = Vec::new();
        for (at, &path) in self.added.iter().enumerate() {
            if self
                .within
                .as_deref()
                .is_some_and(|folder| !is_inside(path, folder))
            {
                continue;
            }
            let Some(name) = path.file_name().map(OsStr::as_encoded_bytes) else {
                continue;
            };
            let mut reach = vec![0; self.terms.len()];
            let above = path
                .parent()
                .and_then(|folder| folder.strip_prefix(root).ok());
            let depth = above.map_or(0, |folders| {
                folders
                    .iter()
                    .inspect(|folder| {
                        fold::fold_into(folder.as_encoded_bytes(), &mut folded);
                        self.terms.enter(&mut reach, &folded);
                    })
                    .count()
            });
            fold::fold_into(name, &mut folded);
            let starts_word = |at| fold::starts_word(name, &folded, at);
            let Some(met) = self.terms.meet(&reach, &folded, loose, starts_word) else {
                continue;
            };
            if loose && !met.loosely {
                continue;
            }
            let hidden = is_hidden(root, path);
            ranked.push((
                met.by_folders,
                met.found.kind,
                hidden && self.hidden_last,
                met.found.cost,
                folded.len(),
                depth,
                Source::Added(at),
            ));
        }
        ranked
    }

    fn best(&mut self, mut ranked: Vec<Rank>) -> Vec<Hit> {
        if self.overlay.is_some_and(Overlay::moves) {
            return self.best_placed(ranked);
        }
        let changed = self.overlay.is_some_and(|overlay| !overlay.is_empty());
        if !changed && ranked.len() > self.limit {
            ranked.select_nth_unstable(self.limit);
            ranked.truncate(self.limit);
        }
        ranked.sort_unstable();
        let mut hits = Vec::new();
        for (_, kind, .., source) in ranked {
            if hits.len() == self.limit {
                break;
            }
            let Some(path) = self.path(source) else {
                continue;
            };
            if self.shown.insert(path.clone()) {
                hits.push(Hit { path, kind });
            }
        }
        hits
    }

    fn best_placed(&mut self, ranked: Vec<Rank>) -> Vec<Hit> {
        let root = self.index.root();
        let within = self.within.as_deref();
        let mut placed: Vec<(Rank, PathBuf)> = ranked
            .into_iter()
            .filter_map(|(by_folders, kind, _, cost, length, depth, source)| {
                let path = self.path(source)?;
                if within.is_some_and(|folder| !is_inside(&path, folder)) {
                    return None;
                }
                let hidden = if self.hidden_last {
                    is_hidden(root, &path)
                } else {
                    false
                };
                Some((
                    (by_folders, kind, hidden, cost, length, depth, source),
                    path,
                ))
            })
            .collect();
        placed.sort_unstable_by_key(|(rank, _)| *rank);
        let mut hits = Vec::new();
        for ((_, kind, ..), path) in placed {
            if hits.len() == self.limit {
                break;
            }
            if self.shown.insert(path.clone()) {
                hits.push(Hit { path, kind });
            }
        }
        hits
    }

    fn path(&self, source: Source) -> Option<PathBuf> {
        match (source, self.overlay) {
            (Source::Row(row), None) => self.index.path(row),
            (Source::Row(row), Some(overlay)) => overlay.place(self.index.path(row)?),
            (Source::Added(at), _) => self.added.get(at).map(|&path| path.to_owned()),
        }
    }
}

fn is_inside(path: &Path, folder: &Path) -> bool {
    path != folder && path.starts_with(folder)
}

fn is_hidden(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root)
        .unwrap_or(path)
        .iter()
        .any(|part| part.as_encoded_bytes().first() == Some(&b'.'))
}

#[cfg(test)]
mod tests;
