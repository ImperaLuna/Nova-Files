//! File operations, run as jobs: each on a thread of its own, reported to
//! every client.
//!
//! [`Jobs::start`] gives the job an id, tells the client that asked, and
//! starts the job's thread. While it runs, every connected client hears
//! its progress, and any client may pause, resume or cancel it (see
//! `control`); the job checks before each change to the disk. When it
//! ends, every client hears how and which folders changed (see
//! `audience`). Nothing a job does ever replaces an existing entry unless
//! the user chose that: what stands in a copy's or move's way, or what a
//! delete may not remove, is a conflict the job holds at until a client
//! answers it. Copies, moves, deletes and trashing write a journal of
//! their steps (see `journal`); trashing moves entries to the desktop's
//! trash (see `trash`).
//!
//! A job running or paused keeps the core up: the core's loop asks
//! [`Jobs::busy`], and a job that ends wakes the loop so its idle wait
//! starts from then.

mod access;
mod attrs;
mod audience;
mod clash;
mod control;
mod copy_file;
mod delete;
mod failure;
mod job;
mod journal;
mod keep_both;
mod left;
mod make;
mod name;
mod one_step;
mod place;
mod plan;
mod progress;
mod running;
mod stop;
mod sync;
#[cfg(test)]
pub mod test_folder;
mod transfer;
mod trash;

use std::collections::HashMap;
use std::fs;
use std::io;
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
#[cfg(test)]
pub use test_folder::TestFolder;

use crate::jobs::Client;
use access::change_access;
use delete::delete;
use job::Job;
use left::Left;
use make::How;
use progress::Reporter;
use running::Running;
use transfer::transfer;
use trash::trash;

const JOB_THREAD: &str = "nova-files-job";
const RECOVER_THREAD: &str = "nova-files-recover";
const FORGET_THREAD: &str = "nova-files-forget";

/// What a job ended with, and the folders whose entries it changed.
pub type Ended = (Outcome, Vec<PathBuf>);

type Registry = Arc<Mutex<HashMap<JobId, Arc<Job>>>>;
type Interrupted = Arc<Mutex<HashMap<JobId, Left>>>;

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
    /// The home trash's folder; `None` without a data folder.
    home_trash: Option<PathBuf>,
    /// Jobs a crash cut short, waiting to be resumed or forgotten. Added
    /// to under the lock of `running`, so a client never misses one.
    interrupted: Interrupted,
    /// Wakes the core's loop when a job ends.
    wake: Connector,
}

impl Jobs {
    /// Jobs whose journals go under the state folder `state` and whose
    /// home trash is under the data folder `data`. Jobs a crash cut
    /// short are found there, cleaned up, and wait as interrupted; on a
    /// thread of their own, as the disk may be slow.
    pub fn new(state: Option<&Path>, data: Option<&Path>, wake: Connector) -> Self {
        let journals = state.map(|state| state.join(journal::FOLDER));
        let home_trash = data.map(|data| data.join(crate::trash::HOME_TRASH));
        let jobs = Self {
            next: AtomicU64::new(first_id()),
            audience: Audience::default(),
            running: Registry::default(),
            journals,
            home_trash,
            interrupted: Interrupted::default(),
            wake,
        };
        if let Some(folder) = jobs.journals.clone() {
            jobs.recover(folder);
        }
        jobs
    }

    /// Finds the jobs a crash cut short in `folder`, cleans them up, and
    /// tells clients of each.
    fn recover(&self, folder: PathBuf) {
        let (running, interrupted, audience) = (
            Arc::clone(&self.running),
            Arc::clone(&self.interrupted),
            self.audience.clone(),
        );
        let spawned = thread::Builder::new()
            .name(RECOVER_THREAD.to_owned())
            .spawn(move || {
                let found = left::recover(&folder);
                let running = lock(&running);
                let mut interrupted = lock(&interrupted);
                for left in found {
                    audience.tell(&Event::JobInterrupted {
                        job: left.job,
                        operation: left.operation.clone(),
                    });
                    interrupted.insert(left.job, left);
                }
                drop(interrupted);
                drop(running);
            });
        if let Err(error) = spawned {
            log::warn!("can't look for jobs a crash cut short: {error}");
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
        for left in lock(&self.interrupted).values() {
            let event = Event::JobInterrupted {
                job: left.job,
                operation: left.operation.clone(),
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
        self.spawn(lock(&self.running), job, operation, None);
        told
    }

    /// Runs `operation` as job `job` on a thread of its own, resuming it
    /// after a crash when it `left` something. The job joins `registry`,
    /// the jobs' lock held, before that is let go.
    fn spawn(
        &self,
        mut registry: MutexGuard<'_, HashMap<JobId, Arc<Job>>>,
        job: JobId,
        operation: Operation,
        left: Option<Left>,
    ) {
        let shared = Arc::new(Job::new(operation.clone()));
        registry.insert(job, Arc::clone(&shared));
        drop(registry);
        let failed_path = first_path(&operation);
        let reporter = Reporter::new(
            self.audience.clone(),
            Arc::clone(&self.running),
            job,
            Arc::clone(&shared),
        );
        let mut running = Running::new(job, shared, reporter, self.journals.clone());
        // New hidden names for a resumed job, never a crash's.
        let resume = left.map(|left| (left, self.next.fetch_add(1, Ordering::Relaxed)));
        let (registry, audience, wake, home_trash) = (
            Arc::clone(&self.running),
            self.audience.clone(),
            self.wake.clone(),
            self.home_trash.clone(),
        );
        let spawned = thread::Builder::new()
            .name(JOB_THREAD.to_owned())
            .spawn(move || {
                let operation = match resume {
                    Some((left, names)) => {
                        let rest = still_to_do(&left.operation);
                        running.resuming(left, names);
                        rest
                    }
                    None => operation,
                };
                let ended = run(&mut running, &operation, home_trash.as_deref());
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

    /// Lets held job `job` go on, or starts interrupted job `job` again.
    pub fn resume(&self, job: JobId) {
        let running = lock(&self.running);
        if running
            .get(&job)
            .is_some_and(|shared| shared.control.resume())
        {
            self.audience.tell(&Event::JobResumed(job));
            return;
        }
        let Some(left) = lock(&self.interrupted).remove(&job) else {
            return;
        };
        self.audience.tell(&Event::JobResumed(job));
        let operation = left.operation.clone();
        self.spawn(running, job, operation, Some(left));
    }

    /// Drops interrupted job `job`, and its journal. The journal goes on
    /// a thread of its own, so a slow disk never holds up the core's
    /// loop; clients hear the job ended once it has. A journal that
    /// can't be removed keeps its job, interrupted, and clients hear so
    /// again.
    pub fn forget(&self, job: JobId) {
        let Some(left) = lock(&self.interrupted).remove(&job) else {
            return;
        };
        let (registry, interrupted, audience) = (
            Arc::clone(&self.running),
            Arc::clone(&self.interrupted),
            self.audience.clone(),
        );
        let forgotten = move || {
            let removed = fs::remove_file(&left.journal);
            let running = lock(&registry);
            match removed {
                Err(error) if error.kind() != io::ErrorKind::NotFound => {
                    log::warn!("can't remove {}: {error}", left.journal.display());
                    // Back before clients hear, so an answer finds it.
                    let operation = left.operation.clone();
                    lock(&interrupted).insert(job, left);
                    audience.tell(&Event::JobInterrupted { job, operation });
                }
                // Held until now, so no other core takes the job meanwhile.
                _ => {
                    drop(left);
                    audience.tell(&Event::JobFinished {
                        job,
                        outcome: Outcome::Cancelled,
                        changed: Vec::new(),
                    });
                }
            }
            drop(running);
        };
        if let Err(error) = thread::Builder::new()
            .name(FORGET_THREAD.to_owned())
            .spawn(forgotten)
        {
            log::warn!("can't forget job {}: {error}", job.0);
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

fn run(running: &mut Running, operation: &Operation, home_trash: Option<&Path>) -> Ended {
    match operation {
        Operation::Rename { path, name } => one_step::rename(path, name),
        Operation::NewFolder { inside, name } => one_step::new_folder(inside, name),
        Operation::Copy { sources, into } => transfer(running, sources, into, How::Copy),
        Operation::Move { sources, into } => transfer(running, sources, into, How::Move),
        Operation::Delete { paths } => delete(running, paths),
        Operation::ChangeAccess { paths, change } => change_access(running, paths, change),
        Operation::Trash { paths } => trash(running, paths, home_trash),
    }
}

/// What is left of `operation` after a crash: a move, delete or trash
/// leaves out the sources already gone (moved whole, deleted, or in the
/// trash); a copy does
/// all of it again, taking what it made as done. A source that can't be
/// looked at stays, so the job reports it.
fn still_to_do(operation: &Operation) -> Operation {
    let there = |paths: &[PathBuf]| -> Vec<PathBuf> {
        paths
            .iter()
            .filter(|path| {
                !matches!(
                    path.symlink_metadata(),
                    Err(error) if error.kind() == io::ErrorKind::NotFound
                )
            })
            .cloned()
            .collect()
    };
    match operation {
        Operation::Move { sources, into } => Operation::Move {
            sources: there(sources),
            into: into.clone(),
        },
        Operation::Delete { paths } => Operation::Delete {
            paths: there(paths),
        },
        Operation::Trash { paths } => Operation::Trash {
            paths: there(paths),
        },
        other => other.clone(),
    }
}

/// The path a job that couldn't start failed at.
fn first_path(operation: &Operation) -> PathBuf {
    match operation {
        Operation::Rename { path, .. } => path.clone(),
        Operation::NewFolder { inside, name } => inside.join(name),
        Operation::Copy { sources, into } | Operation::Move { sources, into } => {
            sources.first().unwrap_or(into).clone()
        }
        Operation::Delete { paths }
        | Operation::Trash { paths }
        | Operation::ChangeAccess { paths, .. } => paths.first().cloned().unwrap_or_default(),
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

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::test_folder::TestFolder;

    #[test]
    fn a_resumed_delete_keeps_a_source_it_cant_look_at() {
        let folder = TestFolder::new("ops-still-to-do");
        let file = folder.file("file", "f");
        // Under a file: not missing, but it can't be looked at.
        let unreachable = file.join("x");
        let gone = folder.path().join("gone");
        let delete = Operation::Delete {
            paths: vec![file.clone(), unreachable.clone(), gone],
        };

        let left = still_to_do(&delete);

        assert_eq!(
            left,
            Operation::Delete {
                paths: vec![file, unreachable]
            }
        );
    }

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
