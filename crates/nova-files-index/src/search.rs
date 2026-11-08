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
//! A row ranks by its kind, then whether it is hidden, then how early (or,
//! for letters in order, how close together) its words matched, then by
//! the shorter name. Hidden rows (a name starting with `.`, or anything in
//! a folder whose name does) rank below the rest of their kind, unless the
//! query starts with `.`. Only entry names are searched, never the folders
//! above them, and never row 0, the indexed folder itself.

mod fold;
mod matching;

use crate::reader::Index;
use crate::{Kind, RowId};

pub use matching::MatchKind;

/// One result: its row and how it matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub row: RowId,
    pub kind: MatchKind,
}

/// A search in progress; each [`Iterator::next`] runs the next stage.
pub struct Search<'a> {
    index: &'a Index,
    words: Vec<Vec<u8>>,
    /// Whether hidden rows rank below the rest of their kind.
    hidden_last: bool,
    limit: usize,
    /// The rows the first stage matched, which the second leaves out.
    found: Vec<bool>,
    stage: Stage,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    AsTyped,
    Loose,
    Done,
}

/// What orders hits: lower first.
type Rank = (MatchKind, bool, usize, usize, RowId);

impl Index {
    /// Searches the names for `query`, a word or several separated by
    /// spaces, keeping the best `limit` hits of each stage.
    pub fn search(&self, query: &str, limit: usize) -> Search<'_> {
        let mut words: Vec<Vec<u8>> = Vec::new();
        for word in query.split_whitespace() {
            let mut folded = Vec::new();
            fold::fold_into(word.as_bytes(), &mut folded);
            words.push(folded);
        }
        let stage = if words.is_empty() || limit == 0 {
            Stage::Done
        } else {
            Stage::AsTyped
        };
        Search {
            index: self,
            words,
            hidden_last: !query.trim_start().starts_with('.'),
            limit,
            found: Vec::new(),
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
            Stage::AsTyped => Some(self.scan(false)),
            Stage::Loose => Some(self.scan(true)),
            Stage::Done => None,
        }
    }
}

impl Search<'_> {
    /// One pass over every row but the first, best `limit` hits first.
    fn scan(&mut self, loose: bool) -> Vec<Hit> {
        let index = self.index;
        let rows = index.rows();
        if !loose {
            self.found = vec![false; usize::try_from(rows).unwrap_or_default()];
        }
        let mut ranked: Vec<Rank> = Vec::new();
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
                .found
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
                row,
            ));
        }
        best(ranked, self.limit)
    }
}

/// The best `limit` of `ranked`, best first.
fn best(mut ranked: Vec<Rank>, limit: usize) -> Vec<Hit> {
    if ranked.len() > limit {
        ranked.select_nth_unstable(limit);
        ranked.truncate(limit);
    }
    ranked.sort_unstable();
    ranked
        .into_iter()
        .map(|(kind, .., row)| Hit { row, kind })
        .collect()
}
