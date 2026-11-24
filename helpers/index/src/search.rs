mod matching;
mod placed;
mod scan;
mod terms;

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::RowId;
use crate::overlay::Overlay;
use crate::reader::Index;

pub use matching::MatchKind;
use scan::{Rank, Scan, Source};
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
    query: String,
    kept: Option<&'a Kept>,
    stop: Option<&'a (dyn Fn() -> bool + Sync)>,
    typed_rows: Option<Vec<u32>>,
    loose_rows: Option<Vec<u32>>,
    moved: &'a [Range<u32>],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    query: String,
    within: Option<PathBuf>,
    typed: Vec<u32>,
    any: Option<Vec<u32>>,
}

const KEPT_ROWS_MAX: usize = 262_144;

pub const STAGES: usize = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    AsTyped,
    Loose,
    Done,
}

impl Index {
    pub fn search(&self, query: &str, limit: usize) -> Search<'_> {
        self.searching(query, limit, None)
    }

    pub fn search_now<'a>(&'a self, query: &str, limit: usize, overlay: &'a Overlay) -> Search<'a> {
        self.searching(query, limit, Some(overlay))
    }

    fn moved_rows(&self, overlay: &Overlay) -> Vec<Range<u32>> {
        let mut inside: HashMap<&Path, HashMap<&OsStr, RowId>> = HashMap::new();
        let mut moved: Vec<Range<u32>> = overlay
            .moved_from()
            .filter_map(|from| {
                let (folder, name) = (from.parent()?, from.file_name()?);
                let rows = match inside.entry(folder) {
                    Entry::Occupied(known) => known.into_mut(),
                    Entry::Vacant(new) => new.insert(self.named_in(folder)),
                };
                let row = *rows.get(name)?;
                Some(row.0..self.end(row)?.0)
            })
            .collect();
        moved.sort_unstable_by_key(|rows| (rows.start, std::cmp::Reverse(rows.end)));
        moved.dedup_by(|inner, outer| inner.end <= outer.end);
        moved
    }

    fn named_in(&self, folder: &Path) -> HashMap<&OsStr, RowId> {
        self.row_of(folder)
            .into_iter()
            .flat_map(|folder| self.children(folder))
            .filter_map(|row| Some((self.name(row)?, row)))
            .collect()
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
            query: query.to_owned(),
            kept: None,
            stop: None,
            typed_rows: None,
            loose_rows: None,
            moved: overlay.map_or(&[], |overlay| {
                overlay.moved_rows(|| self.moved_rows(overlay))
            }),
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

impl<'a> Search<'a> {
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

    #[must_use]
    pub fn from(mut self, kept: &'a Kept) -> Self {
        if self.terms.narrows(&Terms::parse(&kept.query)) {
            self.kept = Some(kept);
        }
        self
    }

    #[must_use]
    pub fn stopping(mut self, stop: &'a (dyn Fn() -> bool + Sync)) -> Self {
        self.stop = Some(stop);
        self
    }

    pub fn keep(self) -> Option<Kept> {
        let typed = self.typed_rows.filter(|rows| rows.len() <= KEPT_ROWS_MAX)?;
        let any = self
            .loose_rows
            .map(|loose| merged(&typed, &loose))
            .filter(|rows| rows.len() <= KEPT_ROWS_MAX);
        Some(Kept {
            query: self.query,
            within: self.within,
            typed,
            any,
        })
    }

    fn stage_hits(&mut self, loose: bool) -> Vec<Hit> {
        let mut ranked = self.scan_rows(loose);
        ranked.extend(self.scan_added(loose));
        self.best(ranked)
    }

    fn scan_rows(&mut self, loose: bool) -> Vec<Rank> {
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
            stop: self.stop,
            overlay: self.overlay,
            moved: self.moved,
        };
        let kept = self.kept.filter(|kept| kept.within == self.within);
        let pool = kept.and_then(|kept| {
            if loose {
                kept.any.as_deref()
            } else {
                Some(kept.typed.as_slice())
            }
        });
        let scanned = match pool {
            Some(rows) => scan.among(rows),
            None => scan.over(self.rows.clone()),
        };
        let rows = (!scanned.stopped).then_some(scanned.rows);
        if loose {
            self.loose_rows = rows;
        } else {
            self.typed_rows = rows;
        }
        scanned.ranked
    }

    fn scan_added(&self, loose: bool) -> Vec<Rank> {
        let root = self.index.root();
        let mut ranked = Vec::new();
        for (at, &path) in self.added.iter().enumerate() {
            if self
                .within
                .as_deref()
                .is_some_and(|folder| !is_inside(path, folder))
            {
                continue;
            }
            let Some(placed) = placed::meet(&self.terms, root, path, loose) else {
                continue;
            };
            let hidden = is_hidden(root, path);
            ranked.push((
                placed.met.by_folders,
                placed.met.found.kind,
                hidden && self.hidden_last,
                placed.met.found.cost,
                placed.length,
                placed.depth,
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

fn merged(one: &[u32], other: &[u32]) -> Vec<u32> {
    let mut all = Vec::with_capacity(one.len() + other.len());
    let (mut one, mut other) = (one.iter().peekable(), other.iter().peekable());
    while let (Some(&&left), Some(&&right)) = (one.peek(), other.peek()) {
        all.push(left.min(right));
        if left <= right {
            one.next();
        }
        if right <= left {
            other.next();
        }
    }
    all.extend(one);
    all.extend(other);
    all
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
