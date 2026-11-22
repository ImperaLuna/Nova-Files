use std::time::{Duration, Instant};

use nova_files_api::{Conflict, Operation, Progress};

use super::job_words::Words;
use super::words::{ITEM, ITEMS, PART_GAP, count, name_of};

pub const STEADY_AFTER: Duration = Duration::from_secs(1);

const SECONDS_IN_MINUTE: u64 = 60;
const MINUTES_IN_HOUR: u64 = 60;
const WAITING: &str = "waiting";
const PAUSED: &str = "paused";
const LEFT: &str = "left";
const WORKING: &str = "Working";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Start {
    pub at: Instant,
    pub done: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Running {
    pub operation: Operation,
    pub progress: Progress,
    pub started: Instant,
    pub counted_from: Option<Start>,
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

    pub const fn advanced(&mut self, progress: Progress, now: Instant) {
        self.progress = progress;
        if self.counted_from.is_none() && work(&progress).1 > 0 {
            self.counted_from = Some(Start {
                at: now,
                done: work(&progress).0,
            });
        }
    }

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

    pub fn line(&self, now: Instant, words: &Words) -> String {
        let (verb, after) = doing(&self.operation, words);
        let sources = sources(&self.operation, words);
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

const fn work(progress: &Progress) -> (u64, u64) {
    if progress.total_bytes > 0 {
        (progress.done_bytes, progress.total_bytes)
    } else {
        (progress.done_items, progress.total_items)
    }
}

pub fn left_text(left: Duration) -> String {
    format!("{} {LEFT}", short_left(left))
}

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

fn doing(operation: &Operation, words: &Words) -> (&'static str, &'static str) {
    match operation {
        Operation::Feature(_) => words
            .of(operation)
            .and_then(|words| words.doing(operation))
            .map_or((WORKING, ""), |doing| (doing.verb, doing.after_items)),
        Operation::Undo { .. } => ("Undoing", ""),
    }
}

pub fn sources<'a>(operation: &'a Operation, words: &Words) -> &'a [std::path::PathBuf] {
    match operation {
        Operation::Feature(_) => words
            .of(operation)
            .and_then(|words| words.sources(operation))
            .unwrap_or_default(),
        Operation::Undo { .. } => &[],
    }
}

#[cfg(test)]
mod tests;
