use std::num::NonZero;
use std::ops::Range;
use std::panic::resume_unwind;
use std::thread;

use super::matching::MatchKind;
use super::terms::Terms;
use crate::reader::Index;
use crate::{Kind, RowId};

const SCAN_THREADS_MAX: usize = 16;
pub const ROWS_PER_THREAD_MIN: usize = 32_768;
const KEEP_SLACK: usize = 4;
const KEEP_MIN: usize = 1024;
const ROWS_BETWEEN_STOP_CHECKS: u32 = 8192;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    Row(RowId),
    Added(usize),
}

pub type Rank = (usize, MatchKind, bool, usize, usize, usize, Source);

pub struct Scan<'a> {
    pub index: &'a Index,
    pub terms: &'a Terms,
    pub loose: bool,
    pub all_hidden: bool,
    pub hidden_last: bool,
    pub keep: Option<usize>,
    pub stop: Option<&'a (dyn Fn() -> bool + Sync)>,
}

#[derive(Default)]
pub struct Scanned {
    pub ranked: Vec<Rank>,
    pub rows: Vec<u32>,
    pub stopped: bool,
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

impl Scanned {
    fn take(&mut self, rank: Rank, row: RowId, keep: Option<usize>) {
        self.ranked.push(rank);
        self.rows.push(row.0);
        if let Some(keep) = keep
            && self.ranked.len() >= keep.saturating_mul(KEEP_SLACK).max(KEEP_MIN)
        {
            self.ranked.select_nth_unstable(keep);
            self.ranked.truncate(keep);
        }
    }

    fn joined(parts: impl Iterator<Item = Self>) -> Self {
        parts.fold(Self::default(), |mut all, part| {
            all.ranked.extend(part.ranked);
            all.rows.extend(part.rows);
            all.stopped |= part.stopped;
            all
        })
    }
}

impl Scan<'_> {
    pub fn over(&self, rows: Range<u32>) -> Scanned {
        let count = usize::try_from(rows.end.saturating_sub(rows.start)).unwrap_or_default();
        let share = u32::try_from(count.div_ceil(threads_for(count))).unwrap_or(u32::MAX);
        let parts = (0..threads_for(count)).scan(rows.start, |start, _| {
            let end = start.saturating_add(share).min(rows.end);
            Some(std::mem::replace(start, end)..end)
        });
        spread(parts, |part| self.run(part))
    }

    pub fn among(&self, rows: &[u32]) -> Scanned {
        let share = rows.len().div_ceil(threads_for(rows.len())).max(1);
        spread(rows.chunks(share), |part| self.some(part))
    }

    fn stopped(&self) -> bool {
        self.stop.is_some_and(|stop| stop())
    }

    fn run(&self, rows: Range<u32>) -> Scanned {
        let index = self.index;
        let mut scanned = Scanned::default();
        let mut open = Open::above(self, RowId(rows.start));
        for (row, seen) in rows.map(RowId).zip(0_u32..) {
            if seen.is_multiple_of(ROWS_BETWEEN_STOP_CHECKS) && self.stopped() {
                scanned.stopped = true;
                break;
            }
            open.leave_before(row);
            if let Some(rank) = self.rank(row, &open) {
                scanned.take(rank, row, self.keep);
            }
            if index.kind(row) == Some(Kind::Folder) {
                open.enter(self, row);
            }
        }
        scanned
    }

    fn some(&self, rows: &[u32]) -> Scanned {
        let index = self.index;
        let mut scanned = Scanned::default();
        let mut open: Option<(Option<RowId>, Open)> = None;
        for (&row, seen) in rows.iter().zip(0_u32..) {
            if seen.is_multiple_of(ROWS_BETWEEN_STOP_CHECKS) && self.stopped() {
                scanned.stopped = true;
                break;
            }
            let row = RowId(row);
            let folder = index.parent(row);
            let (_, above) = open
                .take_if(|(of, _)| *of == folder)
                .unwrap_or_else(|| (folder, Open::above(self, row)));
            if let Some(rank) = self.rank(row, &above) {
                scanned.take(rank, row, self.keep);
            }
            open = Some((folder, above));
        }
        scanned
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

fn threads_for(rows: usize) -> usize {
    thread::available_parallelism()
        .map_or(1, NonZero::get)
        .min(SCAN_THREADS_MAX)
        .min(rows / ROWS_PER_THREAD_MIN)
        .max(1)
}

fn spread<P: Send>(parts: impl Iterator<Item = P>, scan: impl Fn(P) -> Scanned + Sync) -> Scanned {
    thread::scope(|scope| {
        let running: Vec<_> = parts
            .map(|part| {
                let scan = &scan;
                scope.spawn(move || scan(part))
            })
            .collect();
        Scanned::joined(
            running
                .into_iter()
                .map(|part| part.join().unwrap_or_else(|panic| resume_unwind(panic))),
        )
    })
}
