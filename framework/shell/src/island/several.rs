use std::time::Instant;

use nova_files_api::{Operation, Tally};

use super::job_words::Words;
use super::progress::{Running, short_left, sources};
use super::words::{ITEM, ITEMS, PART_GAP, count, name_of, what, with_answers};

pub fn line(jobs: &[&Running], now: Instant, words: &Words) -> (String, String) {
    let all = |kind: fn(&Operation) -> bool| jobs.iter().all(|job| kind(&job.operation));
    let grouped = |job: &&Running| {
        words
            .of(&job.operation)
            .and_then(|words| words.grouped(&job.operation))
    };
    let feature = jobs
        .first()
        .and_then(grouped)
        .filter(|first| jobs.iter().all(|job| grouped(job) == Some(*first)));
    let shared = if all(|operation| matches!(operation, Operation::Copy { .. })) {
        ("copy", "copies")
    } else if all(|operation| matches!(operation, Operation::Move { .. })) {
        ("move", "moves")
    } else {
        ("job", "jobs")
    };
    let (one, many) = feature.map_or(shared, |grouped| (grouped.one, grouped.many));
    let many = count(jobs.len(), one, many);
    let left: Option<Vec<_>> = jobs.iter().map(|job| job.time_left(now)).collect();
    let rest = left
        .and_then(|left| left.into_iter().max())
        .map(|left| format!("{PART_GAP}{}", super::progress::left_text(left)))
        .unwrap_or_default();
    (many, rest)
}

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

pub fn title(job: &Running, now: Instant, words: &Words) -> String {
    if let Some(title) = words
        .of(&job.operation)
        .and_then(|words| words.title(&job.operation))
    {
        return title;
    }
    match &job.operation {
        Operation::Copy { sources, into } | Operation::Move { sources, into } => {
            format!("{} to {}", what(sources), name_of(into))
        }
        _ => job.line(now, words),
    }
}

pub fn left(job: &Running, now: Instant) -> Option<String> {
    job.time_left(now).map(short_left)
}

#[derive(Debug, Default)]
pub struct Batch {
    ended: Vec<(Operation, Tally)>,
}

impl Batch {
    pub fn keep(&mut self, operation: &Operation, tally: Tally) -> bool {
        let kept = matches!(operation, Operation::Copy { .. } | Operation::Move { .. });
        if kept {
            self.ended.push((operation.clone(), tally));
        }
        kept
    }

    pub const fn is_empty(&self) -> bool {
        self.ended.is_empty()
    }

    pub fn note(&mut self) -> String {
        let ended = std::mem::take(&mut self.ended);
        let (mut copied, mut moved, mut answers) = (0, 0, Tally::default());
        let mut any_copy = false;
        for (operation, tally) in &ended {
            let gone = usize::try_from(tally.skipped).unwrap_or(usize::MAX);
            let went = sources(operation, &Words::default())
                .len()
                .saturating_sub(gone);
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
