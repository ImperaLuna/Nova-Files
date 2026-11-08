//! Searching the index by name, in stages, so the first results come fast.
//!
//! [`Index::search`] returns a [`Search`], which yields the results stage
//! by stage, best first within each, at most `limit` per stage:
//!
//! 1. Names that hold every word as typed: [`MatchKind::Exact`],
//!    [`MatchKind::WordStart`] and [`MatchKind::Contains`]. One pass over
//!    the names, comparing bytes.
//! 2. The rest that match once a word may match with its letters in order
//!    or with one typo: [`MatchKind::InOrder`] and [`MatchKind::OneTypo`].
//!
//! A hit ranks by its kind, then whether it is hidden, then how early (or,
//! for letters in order, how close together) its words matched, then by
//! the shorter name. Hidden hits (a name starting with `.`, or anything in
//! a folder whose name does) rank below the rest of their kind, unless the
//! query starts with `.`. Only entry names are searched, never the folders
//! above them, and never row 0, the indexed folder itself.
//!
//! [`Index::search_now`] searches the index as an [`Overlay`] says the disk
//! is now: rows it says are gone are left out, moved ones are reported at
//! their new place, and entries the index doesn't hold are searched too.
//! A path is reported once per search.

mod fold;
mod matching;

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::overlay::Overlay;
use crate::reader::Index;
use crate::{Kind, RowId};

pub use matching::MatchKind;

/// One result: where it is, and how it matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub path: PathBuf,
    pub kind: MatchKind,
}

/// A search in progress; each [`Iterator::next`] runs the next stage.
pub struct Search<'a> {
    index: &'a Index,
    overlay: Option<&'a Overlay>,
    /// The overlay's added entries, in its order.
    added: Vec<&'a Path>,
    words: Vec<Vec<u8>>,
    /// Whether hidden hits rank below the rest of their kind.
    hidden_last: bool,
    limit: usize,
    /// The rows the first stage matched, which the second leaves out.
    rows_found: Vec<bool>,
    /// The same for the overlay's added entries, in its order.
    added_found: Vec<bool>,
    /// Paths already reported.
    shown: HashSet<PathBuf>,
    stage: Stage,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    AsTyped,
    Loose,
    Done,
}

/// A match before it is turned into a [`Hit`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Source {
    Row(RowId),
    /// The overlay's added entry at this position.
    Added(usize),
}

/// What orders matches: lower first.
type Rank = (MatchKind, bool, usize, usize, Source);

impl Index {
    /// Searches the names for `query`, a word or several separated by
    /// spaces, keeping the best `limit` hits of each stage.
    pub fn search(&self, query: &str, limit: usize) -> Search<'_> {
        self.searching(query, limit, None)
    }

    /// Like [`Index::search`], over the index as `overlay` says the disk is
    /// now.
    pub fn search_now<'a>(&'a self, query: &str, limit: usize, overlay: &'a Overlay) -> Search<'a> {
        self.searching(query, limit, Some(overlay))
    }

    fn searching<'a>(
        &'a self,
        query: &str,
        limit: usize,
        overlay: Option<&'a Overlay>,
    ) -> Search<'a> {
        let words: Vec<Vec<u8>> = query
            .split_whitespace()
            .map(|word| {
                let mut folded = Vec::new();
                fold::fold_into(word.as_bytes(), &mut folded);
                folded
            })
            .collect();
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
    fn stage_hits(&mut self, loose: bool) -> Vec<Hit> {
        let mut ranked = self.scan_rows(loose);
        ranked.extend(self.scan_added(loose));
        self.best(ranked)
    }

    /// One pass over every row but the first.
    fn scan_rows(&mut self, loose: bool) -> Vec<Rank> {
        let index = self.index;
        let rows = index.rows();
        if !loose {
            self.rows_found = vec![false; usize::try_from(rows).unwrap_or_default()];
        }
        let mut ranked = Vec::new();
        // Where each hidden folder around the current row ends.
        let mut hidden_until: Vec<RowId> = Vec::new();
        let mut folded = Vec::new();
        for row in (1..rows).map(RowId) {
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

    /// The overlay's added entries, matched like rows.
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

    /// The best `limit` of `ranked` that are still there, best first, each
    /// path once.
    fn best(&mut self, mut ranked: Vec<Rank>) -> Vec<Hit> {
        if self.overlay.is_some_and(Overlay::moves) {
            return self.best_moved(ranked);
        }
        let changed = self.overlay.is_some_and(|overlay| !overlay.is_empty());
        if !changed && ranked.len() > self.limit {
            // Without changes every match is still there: the best `limit`
            // are all that can be shown.
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

    /// [`Search::best`] when entries moved: a move can take a row into or
    /// out of a hidden folder, so every match is placed first and ranked
    /// by where it is now.
    fn best_moved(&mut self, ranked: Vec<Rank>) -> Vec<Hit> {
        let root = self.index.root();
        let mut placed: Vec<(Rank, PathBuf)> = ranked
            .into_iter()
            .filter_map(|(kind, _, cost, length, source)| {
                let path = self.path(source)?;
                let hidden = self.hidden_last && is_hidden(root, &path);
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

    /// Where a match is now, or `None` when it is gone.
    fn path(&self, source: Source) -> Option<PathBuf> {
        match (source, self.overlay) {
            (Source::Row(row), None) => self.index.path(row),
            (Source::Row(row), Some(overlay)) => overlay.place(self.index.path(row)?),
            (Source::Added(at), _) => self.added.get(at).map(|&path| path.to_owned()),
        }
    }
}

/// Whether `path` or a folder it is in, below `root`, starts with a dot.
fn is_hidden(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root)
        .unwrap_or(path)
        .iter()
        .any(|part| part.as_encoded_bytes().first() == Some(&b'.'))
}
