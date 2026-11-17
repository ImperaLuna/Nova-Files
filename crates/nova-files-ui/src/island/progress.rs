//! What the island says while a job runs: "Copying 2 of 5 · 12 s left",
//! and how far its bar is filled.
//!
//! Time left comes from the job's speed since it first knew its totals
//! (the time it spent looking at what to do doesn't count), by bytes when
//! the job moves bytes, else by items. It shows only once the job has run
//! for [`STEADY_AFTER`], so the first guess isn't wild.

use std::time::{Duration, Instant};

use nova_files_api::{Conflict, Operation, Progress};

use super::{ITEM, ITEMS, PART_GAP, count, name_of};

/// How long a job runs at its speed before its time left shows.
pub const STEADY_AFTER: Duration = Duration::from_secs(1);

const SECONDS_IN_MINUTE: u64 = 60;
const MINUTES_IN_HOUR: u64 = 60;
const WAITING: &str = "waiting";
const PAUSED: &str = "paused";
const LEFT: &str = "left";

/// Where a job's count of work stood when it first knew its totals, for
/// its speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Start {
    pub at: Instant,
    pub done: u64,
}

/// One of this window's jobs as the island shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Running {
    pub operation: Operation,
    pub progress: Progress,
    /// When it started, for when the island shows it.
    pub started: Instant,
    /// When its totals were first known, for its speed.
    pub counted_from: Option<Start>,
    /// The conflict it holds at, until answered: the island asks about it.
    pub asking: Option<Conflict>,
    pub paused: bool,
}

impl Running {
    pub const fn new(operation: Operation, started: Instant) -> Self {
        Self {
            operation,
            progress: Progress {
                done_bytes: 0,
                total_bytes: 0,
                done_items: 0,
                total_items: 0,
            },
            started,
            counted_from: None,
            asking: None,
            paused: false,
        }
    }

    /// Takes the job's newest progress, at `now`.
    pub const fn advanced(&mut self, progress: Progress, now: Instant) {
        self.progress = progress;
        if self.counted_from.is_none() && work(&progress).1 > 0 {
            self.counted_from = Some(Start {
                at: now,
                done: work(&progress).0,
            });
        }
    }

    /// How much of the bar is filled, from 0 to 1.
    pub fn fraction(&self) -> f32 {
        let (done, total) = work(&self.progress);
        if total == 0 {
            return 0.0;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "a bar a few hundred pixels long needs no more"
        )]
        let fraction = done.min(total) as f32 / total as f32;
        fraction
    }

    /// The time left at `now`, once the job has run long enough to say.
    pub fn time_left(&self, now: Instant) -> Option<Duration> {
        let start = self.counted_from?;
        let elapsed = now.checked_duration_since(start.at)?;
        let (done, total) = work(&self.progress);
        let gone = done.checked_sub(start.done)?;
        if elapsed < STEADY_AFTER || gone == 0 {
            return None;
        }
        let left = total.saturating_sub(done);
        #[expect(
            clippy::cast_precision_loss,
            reason = "a guess shown in whole seconds needs no more"
        )]
        let seconds = elapsed.as_secs_f64() * left as f64 / gone as f64;
        Some(Duration::from_secs_f64(seconds))
    }

    /// The island's line at `now`.
    pub fn line(&self, now: Instant) -> String {
        let (verb, after) = doing(&self.operation);
        let sources = sources(&self.operation);
        let progress = &self.progress;
        let what = match sources {
            [one] => name_of(one),
            _ if progress.total_items > 1 => format!(
                "{} of {}",
                progress
                    .done_items
                    .saturating_add(1)
                    .min(progress.total_items),
                progress.total_items
            ),
            [] => String::new(),
            many => count(many.len(), ITEM, ITEMS),
        };
        let what = if what.is_empty() {
            what
        } else {
            format!(" {what}")
        };
        let state = if self.asking.is_some() {
            Some(WAITING.to_owned())
        } else if self.paused {
            Some(PAUSED.to_owned())
        } else {
            self.time_left(now).map(left_text)
        };
        let state = state
            .map(|state| format!("{PART_GAP}{state}"))
            .unwrap_or_default();
        format!("{verb}{what}{after}{state}")
    }
}

/// The work done and to do: bytes when the job moves bytes, else items.
const fn work(progress: &Progress) -> (u64, u64) {
    if progress.total_bytes > 0 {
        (progress.done_bytes, progress.total_bytes)
    } else {
        (progress.done_items, progress.total_items)
    }
}

/// "12 s left", "3 min left", "2 h 5 min left", rounded up.
pub fn left_text(left: Duration) -> String {
    format!("{} {LEFT}", short_left(left))
}

/// "12 s", "3 min", "2 h 5 min", rounded up: the time left where "left"
/// goes without saying, as in a row of the island's list of jobs.
pub fn short_left(left: Duration) -> String {
    let seconds = left.as_secs() + u64::from(left.subsec_nanos() > 0);
    if seconds < SECONDS_IN_MINUTE {
        return format!("{} s", seconds.max(1));
    }
    let minutes = seconds.div_ceil(SECONDS_IN_MINUTE);
    if minutes < MINUTES_IN_HOUR {
        return format!("{minutes} min");
    }
    let (hours, minutes) = (minutes / MINUTES_IN_HOUR, minutes % MINUTES_IN_HOUR);
    if minutes == 0 {
        format!("{hours} h")
    } else {
        format!("{hours} h {minutes} min")
    }
}

/// The verb of a running job, and what follows what it works on.
const fn doing(operation: &Operation) -> (&'static str, &'static str) {
    match operation {
        Operation::Copy { .. } => ("Copying", ""),
        Operation::Move { .. } => ("Moving", ""),
        Operation::Trash { .. } => ("Moving", " to Trash"),
        Operation::Restore { .. } => ("Restoring", ""),
        Operation::Delete { .. } | Operation::DeleteFromTrash { .. } | Operation::EmptyTrash => {
            ("Deleting", "")
        }
        Operation::Rename { .. } => ("Renaming", ""),
        Operation::NewFolder { .. } => ("Making", ""),
        Operation::ChangeAccess { .. } => ("Changing", ""),
        Operation::Undo { .. } => ("Undoing", ""),
    }
}

/// What a job works on, as its request named it.
pub fn sources(operation: &Operation) -> &[std::path::PathBuf] {
    match operation {
        Operation::Copy { sources, .. } | Operation::Move { sources, .. } => sources,
        Operation::Trash { paths }
        | Operation::Delete { paths }
        | Operation::ChangeAccess { paths, .. } => paths,
        Operation::Restore { items } | Operation::DeleteFromTrash { items } => items,
        Operation::Rename { path, .. } => std::slice::from_ref(path),
        Operation::NewFolder { inside, .. } => std::slice::from_ref(inside),
        Operation::EmptyTrash | Operation::Undo { .. } => &[],
    }
}

#[cfg(test)]
mod tests;
