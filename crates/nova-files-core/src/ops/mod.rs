//! File operations, run as jobs: each on a thread of its own, reported to
//! every client.
//!
//! [`Jobs::start`] gives the job an id, tells the client that asked, and
//! starts the job's thread. While it runs, every connected client hears
//! its progress, and any client may pause, resume or cancel it (see
//! `control`); the job checks before each change to the disk. When it
//! ends, every client hears how and which folders changed (see
//! `audience`). Nothing a job does ever replaces an existing entry unless
//! the user chose that: what stands in a copy's way is a conflict the job
//! holds at until a client answers it. A copy writes a journal of its
//! steps (see `journal`).
//!
//! A job running or paused keeps the core up: the core's loop asks
//! [`Jobs::busy`], and a job that ends wakes the loop so its idle wait
//! starts from then.

mod attrs;
mod audience;
mod clash;
mod control;
mod copy;
mod copy_file;
mod failure;
mod job;
mod journal;
mod keep_both;
mod name;
mod one_step;
mod place;
mod plan;
mod progress;
mod running;
mod stop;
#[cfg(test)]
mod test_folder;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use nova_files_api::{
    Answer, ClientGone, ClientId, Conflict, Connector, Event, EventSink, Failure, JobId, JobTag,
    Operation, Outcome, Scope,
};

pub use audience::Audience;

use crate::jobs::Client;
use job::Job;
use progress::Reporter;
use running::Running;

const JOB_THREAD: &str = "nova-files-job";

/// What a job ended with, and the folders whose entries it changed.
pub type Ended = (Outcome, Vec<PathBuf>);

type Registry = Arc<Mutex<HashMap<JobId, Arc<Job>>>>;

/// The core's jobs.
pub struct Jobs {
    next: AtomicU64,
    audience: Audience,
    /// The jobs running or paused. Changes to it, and what clients are
    /// told about them, happen under its lock, so a client never hears a
    /// job paused after it heard the job end.
    running: Registry,
    /// Where journals go; `None` without a state folder.
    journals: Option<PathBuf>,
    /// Wakes the core's loop when a job ends.
    wake: Connector,
}

impl Jobs {
    /// Jobs whose journals go under the state folder `state`.
    pub fn new(state: Option<&Path>, wake: Connector) -> Self {
        Self {
            next: AtomicU64::new(first_id()),
            audience: Audience::default(),
            running: Registry::default(),
            journals: state.map(|state| state.join(journal::FOLDER)),
            wake,
        }
    }

    /// The clients that hear about jobs.
    pub const fn audience(&self) -> &Audience {
        &self.audience
    }

    /// `client` watches jobs from now on, first hearing of each job that
    /// already runs.
    pub fn watch(&self, client: ClientId, events: &Arc<dyn EventSink>) {
        // Under the jobs' lock, so no job starts or ends unheard between
        // the list and the watching.
        let running = lock(&self.running);
        for (&job, shared) in running.iter() {
            let event = Event::JobRunning {
                job,
                operation: shared.operation.clone(),
                progress: shared.progress(),
                paused: shared.control.paused(),
                conflict: shared.control.open(),
            };
            events.deliver(event).ok();
        }
        self.audience.join(client, Arc::clone(events));
        drop(running);
    }

    /// Whether a job is running or paused.
    pub fn busy(&self) -> bool {
        !lock(&self.running).is_empty()
    }

    /// Starts `operation` for `client`, which tagged it `tag`.
    ///
    /// # Errors
    /// [`ClientGone`] when `client` stopped listening. The job runs anyway.
    pub fn start(
        &self,
        client: &Client,
        tag: JobTag,
        operation: Operation,
    ) -> Result<(), ClientGone> {
        let job = JobId(self.next.fetch_add(1, Ordering::Relaxed));
        // Before the thread starts, so the client hears of the job before
        // it can end.
        let told = client.deliver(Event::JobStarted { tag, job });
        let shared = Arc::new(Job::new(operation.clone()));
        lock(&self.running).insert(job, Arc::clone(&shared));
        let failed_path = first_path(&operation);
        let reporter = Reporter::new(
            self.audience.clone(),
            Arc::clone(&self.running),
            job,
            Arc::clone(&shared),
        );
        let mut running = Running::new(job, shared, reporter, self.journals.clone());
        let (registry, audience, wake) = (
            Arc::clone(&self.running),
            self.audience.clone(),
            self.wake.clone(),
        );
        let spawned = thread::Builder::new()
            .name(JOB_THREAD.to_owned())
            .spawn(move || {
                let ended = run(&mut running, &operation);
                running.finish();
                finish(&registry, &audience, job, ended);
                // Into the loop's queue, so its idle wait starts again now.
                wake.activity().ok();
            });
        if spawned.is_err() {
            let outcome = Outcome::Failed {
                path: failed_path,
                reason: Failure::Other,
            };
            finish(&self.running, &self.audience, job, (outcome, Vec::new()));
        }
        told
    }

    /// Holds job `job` before its next change to the disk.
    pub fn pause(&self, job: JobId) {
        let running = lock(&self.running);
        if running
            .get(&job)
            .is_some_and(|shared| shared.control.pause())
        {
            self.audience.tell(&Event::JobPaused(job));
        }
    }

    /// Lets held job `job` go on.
    pub fn resume(&self, job: JobId) {
        let running = lock(&self.running);
        if running
            .get(&job)
            .is_some_and(|shared| shared.control.resume())
        {
            self.audience.tell(&Event::JobResumed(job));
        }
    }

    /// Answers `conflict`, which job `job` holds at, with `answer`, for
    /// that conflict alone or for all of its kind as `scope` says. An
    /// answer the conflict doesn't take, or to a conflict not open, is
    /// dropped.
    pub fn answer(&self, job: JobId, conflict: &Conflict, answer: Answer, scope: Scope) {
        let running = lock(&self.running);
        if let Some(shared) = running.get(&job) {
            shared.control.answer(conflict, answer, scope, || {
                self.audience.tell(&Event::ConflictAnswered { job, answer });
            });
        }
        drop(running);
    }

    /// Stops job `job` before its next change to the disk.
    pub fn cancel(&self, job: JobId) {
        if let Some(shared) = lock(&self.running).get(&job) {
            shared.control.cancel();
        }
    }
}

/// The first job id of this run: the time in microseconds, so ids never
/// repeat across runs and a crashed run's journal and part files never
/// share a name with this run's.
fn first_id() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_micros()).unwrap_or(u64::MAX)
        })
}

fn run(running: &mut Running, operation: &Operation) -> Ended {
    match operation {
        Operation::Rename { path, name } => one_step::rename(path, name),
        Operation::NewFolder { inside, name } => one_step::new_folder(inside, name),
        Operation::Copy { sources, into } => copy::copy(running, sources, into),
    }
}

/// The path a job that couldn't start failed at.
fn first_path(operation: &Operation) -> PathBuf {
    match operation {
        Operation::Rename { path, .. } => path.clone(),
        Operation::NewFolder { inside, name } => inside.join(name),
        Operation::Copy { sources, into } => sources.first().unwrap_or(into).clone(),
    }
}

/// Forgets job `job` and tells every client how it ended.
#[expect(
    clippy::significant_drop_tightening,
    reason = "telling under the lock keeps a pause from being heard after the end"
)]
fn finish(registry: &Registry, audience: &Audience, job: JobId, (outcome, changed): Ended) {
    let mut running = lock(registry);
    running.remove(&job);
    audience.tell(&Event::JobFinished {
        job,
        outcome,
        changed,
    });
}

fn lock(registry: &Registry) -> MutexGuard<'_, HashMap<JobId, Arc<Job>>> {
    registry.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_start_from_the_time_so_no_run_repeats_another() {
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros();

        let first = u128::from(first_id());

        assert!(first >= before, "{first} is before {before}");
    }

    #[test]
    fn a_job_that_cannot_start_fails_at_its_first_path() {
        let rename = Operation::Rename {
            path: "/a/b".into(),
            name: "c".into(),
        };
        let folder = Operation::NewFolder {
            inside: "/a".into(),
            name: "new".into(),
        };
        let copy = |sources: Vec<PathBuf>| Operation::Copy {
            sources,
            into: "/into".into(),
        };

        assert_eq!(first_path(&rename), Path::new("/a/b"));
        assert_eq!(first_path(&folder), Path::new("/a/new"));
        assert_eq!(first_path(&copy(vec!["/s".into()])), Path::new("/s"));
        assert_eq!(first_path(&copy(Vec::new())), Path::new("/into"));
    }
}
