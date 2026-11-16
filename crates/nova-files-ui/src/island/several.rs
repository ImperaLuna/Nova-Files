//! Several of the window's jobs at once. They run side by side (the core
//! gives each its own thread), and the island folds them into one line,
//! "**2 copies** · 9 s left", with one bar for all of them. Opened, it
//! lists each job's row above that line: what it works on, its seconds
//! left, its bar and a stop button. Copies and moves that end while
//! others still run leave no note of their own; when the last ends, one
//! note says what they did together (see [`Batch`]).

use std::time::Instant;

use nova_files_api::{Operation, Tally};

use super::progress::{Running, short_left, sources};
use super::{ITEM, ITEMS, PART_GAP, count, name_of, what, with_answers};

/// The folded line: how many jobs, drawn strong, and the time until the
/// last one ends, once every one can say.
pub fn line(jobs: &[&Running], now: Instant) -> (String, String) {
    let all = |kind: fn(&Operation) -> bool| jobs.iter().all(|job| kind(&job.operation));
    let many = if all(|operation| matches!(operation, Operation::Copy { .. })) {
        count(jobs.len(), "copy", "copies")
    } else if all(|operation| matches!(operation, Operation::Move { .. })) {
        count(jobs.len(), "move", "moves")
    } else {
        count(jobs.len(), "job", "jobs")
    };
    let left: Option<Vec<_>> = jobs.iter().map(|job| job.time_left(now)).collect();
    let rest = left
        .and_then(|left| left.into_iter().max())
        .map(|left| format!("{PART_GAP}{}", super::progress::left_text(left)))
        .unwrap_or_default();
    (many, rest)
}

/// How much of the folded line's bar is filled: each job's share, alike.
pub fn fraction(jobs: &[&Running]) -> f32 {
    if jobs.is_empty() {
        return 0.0;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "a handful of jobs on one bar needs no more"
    )]
    let many = jobs.len() as f32;
    jobs.iter().map(|job| job.fraction()).sum::<f32>() / many
}

/// What a job's row says it works on: "notes.txt to Documents", "3
/// items to Documents", or its line for a job that goes nowhere.
pub fn title(job: &Running, now: Instant) -> String {
    match &job.operation {
        Operation::Copy { sources, into } | Operation::Move { sources, into } => {
            format!("{} to {}", what(sources), name_of(into))
        }
        _ => job.line(now),
    }
}

/// A job's seconds left in its row, "6 s", once it can say.
pub fn left(job: &Running, now: Instant) -> Option<String> {
    job.time_left(now).map(short_left)
}

/// The copies and moves that ended while others still ran, for the one
/// note once the last ends.
#[derive(Debug, Default)]
pub struct Batch {
    ended: Vec<(Operation, Tally)>,
}

impl Batch {
    /// Keeps a copy or move that ended done, with its `tally`; `false`
    /// for any other job, which says how it went on its own.
    pub fn keep(&mut self, operation: &Operation, tally: Tally) -> bool {
        let kept = matches!(operation, Operation::Copy { .. } | Operation::Move { .. });
        if kept {
            self.ended.push((operation.clone(), tally));
        }
        kept
    }

    /// Whether it holds none.
    pub const fn is_empty(&self) -> bool {
        self.ended.is_empty()
    }

    /// The one note for all it holds, which it lets go: "Copied 7 items,
    /// moved 2 items, replaced 1".
    pub fn note(&mut self) -> String {
        let ended = std::mem::take(&mut self.ended);
        let (mut copied, mut moved, mut answers) = (0, 0, Tally::default());
        let mut any_copy = false;
        for (operation, tally) in &ended {
            let gone = usize::try_from(tally.skipped).unwrap_or(usize::MAX);
            let went = sources(operation).len().saturating_sub(gone);
            if matches!(operation, Operation::Move { .. }) {
                moved += went;
            } else {
                any_copy = true;
                copied += went;
            }
            answers.replaced += tally.replaced;
            answers.skipped += tally.skipped;
        }
        let mut parts = Vec::new();
        if copied > 0 {
            parts.push(format!("Copied {}", count(copied, ITEM, ITEMS)));
        }
        if moved > 0 {
            let verb = if parts.is_empty() { "Moved" } else { "moved" };
            parts.push(format!("{verb} {}", count(moved, ITEM, ITEMS)));
        }
        if parts.is_empty() {
            let nothing = if any_copy {
                "Nothing copied"
            } else {
                "Nothing moved"
            };
            parts.push(nothing.to_owned());
        }
        with_answers(parts, answers)
    }
}

#[cfg(test)]
mod tests;
