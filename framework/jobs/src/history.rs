use std::collections::VecDeque;
use std::sync::Mutex;

use nova_files_api::{Event, JobId, Operation};

use crate::audience::Audience;
use crate::lock;

pub const KEPT: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Undoable<U> {
    pub job: JobId,
    pub operation: Operation,
    pub undo: U,
}

#[derive(Debug)]
pub struct History<U> {
    jobs: Mutex<VecDeque<Undoable<U>>>,
}

impl<U> Default for History<U> {
    fn default() -> Self {
        Self {
            jobs: Mutex::default(),
        }
    }
}

impl<U> History<U> {
    pub fn add(&self, undoable: Undoable<U>, audience: &Audience) {
        let mut jobs = lock(&self.jobs);
        audience.tell(&Event::Undoable {
            job: undoable.job,
            operation: undoable.operation.clone(),
        });
        let (Ok(at) | Err(at)) = jobs.binary_search_by_key(&undoable.job, |kept| kept.job);
        jobs.insert(at, undoable);
        if jobs.len() > KEPT
            && let Some(gone) = jobs.pop_front()
        {
            audience.tell(&Event::UndoGone(gone.job));
        }
        drop(jobs);
    }

    pub fn take(&self, job: JobId, audience: &Audience) -> Option<Undoable<U>> {
        let mut jobs = lock(&self.jobs);
        let at = jobs.iter().position(|kept| kept.job == job)?;
        let taken = jobs.remove(at)?;
        audience.tell(&Event::UndoGone(job));
        drop(jobs);
        Some(taken)
    }

    pub fn told(&self, then: impl FnOnce(Vec<Event>)) {
        let jobs = lock(&self.jobs);
        then(
            jobs.iter()
                .map(|kept| Event::Undoable {
                    job: kept.job,
                    operation: kept.operation.clone(),
                })
                .collect(),
        );
        drop(jobs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_history_keeps_the_last_jobs_and_gives_each_once() {
        let history = History::default();
        let audience = Audience::default();
        let undoable = |job| Undoable {
            job: JobId(job),
            operation: Operation::Undo { job: JobId(99) },
            undo: job * 2,
        };
        for job in (0..=u64::try_from(KEPT).unwrap()).filter(|job| *job != 3) {
            history.add(undoable(job), &audience);
        }
        history.add(undoable(3), &audience);

        let mut told = Vec::new();
        history.told(|events| told = events);

        let told_of = |job| Event::Undoable {
            job: JobId(job),
            operation: Operation::Undo { job: JobId(99) },
        };
        assert_eq!(told.len(), KEPT);
        assert_eq!(told[0], told_of(1));
        assert_eq!(history.take(JobId(0), &audience), None, "pushed out");
        assert_eq!(told[2], told_of(3));
        assert_eq!(history.take(JobId(3), &audience), Some(undoable(3)));
        assert_eq!(history.take(JobId(3), &audience), None, "taken once");
    }
}
