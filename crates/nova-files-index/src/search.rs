mod fold;
mod matching;

use std::collections::HashSet;
use std::ffi::OsStr;
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::overlay::Overlay;
use crate::reader::Index;
use crate::{Kind, RowId};

pub use matching::MatchKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub path: PathBuf,
    pub kind: MatchKind,
}

pub struct Search<'a> {
    index: &'a Index,
    overlay: Option<&'a Overlay>,
    added: Vec<&'a Path>,
    words: Vec<Vec<u8>>,
    hidden_last: bool,
    limit: usize,
    rows_found: Vec<bool>,
    added_found: Vec<bool>,
    shown: HashSet<PathBuf>,
    within: Option<PathBuf>,
    rows: Range<u32>,
    stage: Stage,
}

pub const STAGES: usize = 2;

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

type Rank = (MatchKind, bool, usize, usize, Source);

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
        let mut words: Vec<Vec<u8>> = Vec::new();
        for word in query.split_whitespace() {
            let mut folded = Vec::new();
            fold::fold_into(word.as_bytes(), &mut folded);
            if !folded.is_empty() {
                words.push(folded);
            }
        }
        let stage = if words.is_empty() || limit == 0 {
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
            words,
            hidden_last: !query.trim_start().starts_with('.'),
            limit,
            rows_found: Vec::new(),
            added_found: Vec::new(),
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
                .and_then(|row| Some(row.0.saturating_add(1)..index.entry(row)?.end.0))
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

    fn scan_rows(&mut self, loose: bool) -> Vec<Rank> {
        let index = self.index;
        let rows = index.rows();
        if !loose {
            self.rows_found = vec![false; usize::try_from(rows).unwrap_or_default()];
        }
        let mut ranked = Vec::new();
        let mut hidden_until: Vec<RowId> = Vec::new();
        if let Some(folder) = &self.within
            && is_hidden(index.root(), folder)
        {
            hidden_until.push(RowId(self.rows.end));
        }
        let mut folded = Vec::new();
        for row in self.rows.clone().map(RowId) {
            while hidden_until.last().is_some_and(|&end| end <= row) {
                hidden_until.pop();
            }
            let (Some(name), Some(entry)) = (index.name(row), index.entry(row)) else {
                continue;
            };
            let name = name.as_encoded_bytes();
            let hidden = !hidden_until.is_empty() || name.first() == Some(&b'.');
            if hidden && entry.kind == Kind::Folder {
                hidden_until.push(entry.end);
            }
            let seen = self
                .rows_found
                .get_mut(usize::try_from(row.0).unwrap_or(usize::MAX));
            if loose && seen.as_deref() == Some(&true) {
                continue;
            }
            fold::fold_into(name, &mut folded);
            let Some(found) = matching::query(name, &folded, &self.words, loose) else {
                continue;
            };
            if let Some(seen) = seen {
                *seen = true;
            }
            ranked.push((
                found.kind,
                hidden && self.hidden_last,
                found.cost,
                name.len(),
                Source::Row(row),
            ));
        }
        ranked
    }

    fn scan_added(&mut self, loose: bool) -> Vec<Rank> {
        let root = self.index.root();
        let mut ranked = Vec::new();
        let mut folded = Vec::new();
        for (at, &path) in self.added.iter().enumerate() {
            if !loose {
                self.added_found.push(false);
            }
            let seen = self.added_found.get_mut(at);
            if loose && seen.as_deref() == Some(&true) {
                continue;
            }
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
            fold::fold_into(name, &mut folded);
            let Some(found) = matching::query(name, &folded, &self.words, loose) else {
                continue;
            };
            if let Some(seen) = seen {
                *seen = true;
            }
            let hidden = is_hidden(root, path);
            ranked.push((
                found.kind,
                hidden && self.hidden_last,
                found.cost,
                name.len(),
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
        for (kind, .., source) in ranked {
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
            .filter_map(|(kind, _, cost, length, source)| {
                let path = self.path(source)?;
                if within.is_some_and(|folder| !is_inside(&path, folder)) {
                    return None;
                }
                let hidden = if self.hidden_last {
                    is_hidden(root, &path)
                } else {
                    false
                };
                Some(((kind, hidden, cost, length, source), path))
            })
            .collect();
        placed.sort_unstable_by_key(|(rank, _)| *rank);
        let mut hits = Vec::new();
        for ((kind, ..), path) in placed {
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
