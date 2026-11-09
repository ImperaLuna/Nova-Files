//! A job's progress, told to every client a few times a second at most.

use std::time::{Duration, Instant};

use nova_files_api::{Conflict, Event, JobId, Progress};

use std::sync::Arc;

use super::audience::Audience;
use super::job::Job;
use super::{Registry, lock};

/// The least time between two progress events of one job.
const EVERY: Duration = Duration::from_millis(100);

pub struct Reporter {
    audience: Audience,
    /// Held while a conflict is raised and told, as while a job pauses or
    /// ends, so no client hears of it out of order.
    registry: Registry,
    /// Where the latest progress is kept for windows that watch later.
    shared: Arc<Job>,
    job: JobId,
    progress: Progress,
    told: Option<Instant>,
}

impl Reporter {
    pub const fn new(audience: Audience, registry: Registry, job: JobId, shared: Arc<Job>) -> Self {
        Self {
            audience,
            registry,
            shared,
            job,
            progress: Progress {
                done_bytes: 0,
                total_bytes: 0,
                done_items: 0,
                total_items: 0,
            },
            told: None,
        }
    }

    /// The job's totals are known: every client hears them now.
    pub fn planned(&mut self, total_bytes: u64, total_items: u64) {
        self.progress.total_bytes = total_bytes;
        self.progress.total_items = total_items;
        self.tell();
    }

    pub fn bytes(&mut self, bytes: u64) {
        self.progress.done_bytes += bytes;
        self.tell_if_due();
    }

    /// `bytes` reported done weren't kept after all: the step that copied
    /// them failed, and it is skipped or tried again.
    pub const fn take_back(&mut self, bytes: u64) {
        self.progress.done_bytes = self.progress.done_bytes.saturating_sub(bytes);
    }

    /// One more item is done; `left` of its bytes were never copied (holes
    /// in a sparse file) but count as done.
    pub fn item(&mut self, left: u64) {
        self.progress.done_items += 1;
        self.progress.done_bytes += left;
        self.tell_if_due();
    }

    fn tell_if_due(&mut self) {
        if self.told.is_none_or(|told| told.elapsed() >= EVERY) {
            self.tell();
        }
    }

    /// The job holds at `conflict` from now on: every client hears it,
    /// with how far the job got.
    pub fn conflict(&mut self, conflict: Conflict) {
        self.tell();
        let registry = lock(&self.registry);
        self.shared.control.raise(conflict.clone());
        self.audience.tell(&Event::JobConflict {
            job: self.job,
            conflict,
        });
        drop(registry);
    }

    /// Tells every client how far the job got, now.
    pub fn tell(&mut self) {
        self.told = Some(Instant::now());
        self.shared.set_progress(self.progress);
        self.audience.tell(&Event::JobProgress {
            job: self.job,
            progress: self.progress,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use nova_files_api::{ClientId, Operation};

    use super::*;

    #[test]
    fn tells_progress_at_most_every_so_often_and_keeps_the_latest() {
        let audience = Audience::default();
        let (sink, heard) = mpsc::channel();
        audience.join(ClientId(1), Arc::new(sink));
        let shared = Arc::new(Job::new(Operation::Copy {
            sources: Vec::new(),
            into: "/".into(),
        }));
        let mut reporter = Reporter::new(audience, Arc::default(), JobId(1), Arc::clone(&shared));
        let progress = |done_bytes, done_items| Progress {
            done_bytes,
            total_bytes: 10,
            done_items,
            total_items: 2,
        };
        let told = |progress| Event::JobProgress {
            job: JobId(1),
            progress,
        };

        reporter.planned(10, 2);
        reporter.bytes(3);
        reporter.item(2);
        assert_eq!(heard.try_iter().collect::<Vec<_>>(), [told(progress(0, 0))]);
        assert_eq!(shared.progress(), progress(0, 0));

        // A wait without a sleep, as the lints ask.
        let (_never, quiet) = mpsc::channel::<()>();
        quiet.recv_timeout(EVERY).ok();
        reporter.item(5);
        assert_eq!(
            heard.try_iter().collect::<Vec<_>>(),
            [told(progress(10, 2))]
        );
        assert_eq!(shared.progress(), progress(10, 2));
    }

    #[test]
    fn bytes_taken_back_are_no_longer_done() {
        let shared = Arc::new(Job::new(Operation::Copy {
            sources: Vec::new(),
            into: "/".into(),
        }));
        let mut reporter = Reporter::new(
            Audience::default(),
            Arc::default(),
            JobId(1),
            Arc::clone(&shared),
        );

        reporter.bytes(5);
        reporter.take_back(3);
        assert_eq!(reporter.progress.done_bytes, 2);
        reporter.take_back(3);
        assert_eq!(reporter.progress.done_bytes, 0, "never below none");
    }
}
