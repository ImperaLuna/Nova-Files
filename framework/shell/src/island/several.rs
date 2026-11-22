use std::time::Instant;

use nova_files_api::{Operation, Tally};

use super::job_words::{Batched, Words};
use super::progress::{Running, short_left, sources};
use super::words::{ITEM, ITEMS, PART_GAP, count, with_answers};

pub fn line(jobs: &[&Running], now: Instant, words: &Words) -> (String, String) {
    let grouped = |job: &&Running| {
        words
            .of(&job.operation)
            .and_then(|words| words.grouped(&job.operation))
    };
    let feature = jobs
        .first()
        .and_then(grouped)
        .filter(|first| jobs.iter().all(|job| grouped(job) == Some(*first)));
    let (one, many) = feature.map_or(("job", "jobs"), |grouped| (grouped.one, grouped.many));
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
    words
        .of(&job.operation)
        .and_then(|words| words.title(&job.operation))
        .unwrap_or_else(|| job.line(now, words))
}

pub fn left(job: &Running, now: Instant) -> Option<String> {
    job.time_left(now).map(short_left)
}

#[derive(Debug, Default)]
pub struct Batch {
    ended: Vec<(usize, Batched, usize, Tally)>,
}

impl Batch {
    pub fn keep(&mut self, operation: &Operation, tally: Tally, words: &Words) -> bool {
        let Some((rank, batched)) = words.batched(operation) else {
            return false;
        };
        let gone = usize::try_from(tally.skipped).unwrap_or(usize::MAX);
        let went = sources(operation, words).len().saturating_sub(gone);
        self.ended.push((rank, batched, went, tally));
        true
    }

    pub const fn is_empty(&self) -> bool {
        self.ended.is_empty()
    }

    pub fn note(&mut self) -> String {
        let mut ended = std::mem::take(&mut self.ended);
        ended.sort_by_key(|(rank, ..)| *rank);
        let mut answers = Tally::default();
        let mut groups: Vec<(Batched, usize)> = Vec::new();
        for (_, batched, went, tally) in &ended {
            match groups.iter_mut().find(|(seen, _)| seen == batched) {
                Some((_, total)) => *total += went,
                None => groups.push((*batched, *went)),
            }
            answers.replaced += tally.replaced;
            answers.skipped += tally.skipped;
        }
        let mut parts = Vec::new();
        for (batched, went) in groups.iter().filter(|(_, went)| *went > 0) {
            let verb = if parts.is_empty() {
                batched.done
            } else {
                batched.done_after
            };
            parts.push(format!("{verb} {}", count(*went, ITEM, ITEMS)));
        }
        if parts.is_empty()
            && let Some((first, _)) = groups.first()
        {
            parts.push(first.nothing.to_owned());
        }
        with_answers(parts, answers)
    }
}

#[cfg(test)]
mod tests;
