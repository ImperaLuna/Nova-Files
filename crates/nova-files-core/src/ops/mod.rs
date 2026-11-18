//! File operations, run as jobs: each on a thread of its own, reported to
//! every client.
//!
//! [`Jobs::start`] gives the job an id, tells the client that asked, and
//! starts the job's thread. A drop of files picks a copy or a move by
//! drive first (see `dropped`). While it runs, every connected client hears
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
mod ahead;
mod attrs;
mod audience;
mod clash;
mod control;
mod copy_file;
mod delete;
mod dropped;
mod extract;
mod failure;
mod folders;
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
mod restore;
mod running;
mod stop;
mod sync;
#[cfg(test)]
pub mod test_folder;
mod transfer;
mod trash;
mod undo;

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
    Operation, Outcome, Scope, Tally,
};

pub use audience::Audience;
pub use dropped::answer as answer_drop_would;
#[cfg(test)]
pub use test_folder::TestFolder;

use crate::archive::Archiver;
use crate::jobs::Client;
use crate::trash::Trashes;
use access::change_access;
use control::{Cancelled, Control};
use delete::delete;
use extract::extract;
use job::Job;
use left::Left;
use make::How;
use progress::Reporter;
use restore::{delete_from_trash, empty_trash, restore};
use running::Running;
use transfer::transfer;
use trash::trash;
use undo::{History, Undo, Undoable, undo_of};

const JOB_THREAD: &str = "nova-files-job";
/// The thread that looks at the drives for a drop.
const DROP_THREAD: &str = "nova-files-drop";
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
    /// Where the user's trashes are.
    trashes: Trashes,
    /// Jobs a crash cut short, waiting to be resumed or forgotten. Added
    /// to under the lock of `running`, so a client never misses one.
    interrupted: Interrupted,
    /// Wakes the core's loop when a job ends.
    wake: Connector,
    /// The jobs that can be undone.
    history: Arc<History>,
    /// Runs the archive helper; `None` when it isn't installed.
    archiver: Option<Archiver>,
}

impl Jobs {
    /// Jobs whose journals go under the state folder `state`, that use
    /// the trashes `trashes`, and that unpack archives with `archiver`.
    /// Jobs a crash cut
    /// short are found there, cleaned up, and wait as interrupted; on a
    /// thread of their own, as the disk may be slow.
    pub fn new(
        state: Option<&Path>,
        trashes: Trashes,
        wake: Connector,
        archiver: Option<Archiver>,
    ) -> Self {
        let journals = state.map(|state| state.join(journal::FOLDER));
        let jobs = Self {
            next: AtomicU64::new(first_id()),
            audience: Audience::default(),
            running: Registry::default(),
            journals,
            trashes,
            interrupted: Interrupted::default(),
            wake,
            history: Arc::default(),
            archiver,
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
        self.history.told(|undoable| {
            for event in undoable {
                events.deliver(event).ok();
            }
            self.audience.join(client, Arc::clone(events));
        });
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
        self.start_for(client.events().as_ref(), tag, operation)
    }

    /// Starts `operation` for the client that hears through `events`,
    /// which tagged it `tag`.
    fn start_for(
        &self,
        events: &dyn EventSink,
        tag: JobTag,
        operation: Operation,
    ) -> Result<(), ClientGone> {
        let job = JobId(self.next.fetch_add(1, Ordering::Relaxed));
        // Before the thread starts, so the client hears of the job before
        // it can end.
        let told = events.deliver(Event::JobStarted {
            tag,
            job,
            operation: operation.clone(),
        });
        self.spawn(lock(&self.running), job, operation, None);
        told
    }

    /// Starts the job dropping `sources` onto the folder `into` runs (see
    /// `dropped`), a move on one drive and a copy across drives, for the
    /// client that hears through `events`. The drives are looked at on a
    /// thread of their own, as a job's disk work is, so a drive that is
    /// slow to answer holds up only this drop. When no thread can start,
    /// the drop copies, which leaves every source where it was.
    pub fn start_drop(
        self: &Arc<Self>,
        events: &Arc<dyn EventSink>,
        tag: JobTag,
        sources: Vec<PathBuf>,
        into: PathBuf,
    ) {
        let jobs = Arc::clone(self);
        // Taken by the thread, or back here when none starts.
        let asked = Arc::new(Mutex::new(Some((sources, into))));
        let taken = Arc::clone(&asked);
        let thread_events = Arc::clone(events);
        let spawned = thread::Builder::new()
            .name(DROP_THREAD.to_owned())
            .spawn(move || {
                let asked = lock(&taken).take();
                if let Some((sources, into)) = asked {
                    let operation = dropped::dropped(sources, into);
                    // A client gone hears nothing; the job runs anyway.
                    jobs.start_for(thread_events.as_ref(), tag, operation).ok();
                }
            });
        let unasked = spawned.is_err().then(|| lock(&asked).take()).flatten();
        if let Some((sources, into)) = unasked {
            let copy = Operation::Copy { sources, into };
            self.start_for(events.as_ref(), tag, copy).ok();
        }
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
        running.home_trash.clone_from(&self.trashes.home);
        // New hidden names for a resumed job, never a crash's.
        let resume = left.map(|left| (left, self.next.fetch_add(1, Ordering::Relaxed)));
        // Taken now, so an undo runs once however often it is asked for;
        // put back unless it ends done.
        let taken = match &operation {
            Operation::Undo { job } => self.history.take(*job, &self.audience),
            _ => None,
        };
        let not_spawned = taken.clone();
        let (registry, audience, wake, history) = (
            Arc::clone(&self.running),
            self.audience.clone(),
            self.wake.clone(),
            Arc::clone(&self.history),
        );
        let tools = Tools {
            trashes: self.trashes.clone(),
            archiver: self.archiver.clone(),
        };
        let spawned = thread::Builder::new()
            .name(JOB_THREAD.to_owned())
            .spawn(move || {
                let undo = taken.as_ref().map(|taken| taken.undo.clone());
                let ended = match resumed(&mut running, resume) {
                    None => run(&mut running, &operation, &tools, undo),
                    Some(Ok(rest)) => run(&mut running, &rest, &tools, undo),
                    Some(Err(Cancelled)) => (Outcome::Cancelled, Vec::new()),
                };
                let (tally, undoable) = after(&mut running, &operation, &ended, &tools.trashes);
                running.finish();
                // Before clients hear the job ended, so an undo asked for
                // then finds it.
                if let Some(undo) = undoable {
                    history.add(
                        Undoable {
                            job,
                            operation,
                            undo,
                        },
                        &audience,
                    );
                }
                // Back unless done, or nothing was left to take back.
                let spent = matches!(
                    ended.0,
                    Outcome::Done
                        | Outcome::Failed {
                            reason: Failure::NotFound,
                            ..
                        }
                );
                if let Some(taken) = taken.filter(|_| !spent) {
                    history.add(taken, &audience);
                }
                finish(&registry, &audience, job, ended, tally);
                // Into the loop's queue, so its idle wait starts again now.
                wake.activity().ok();
            });
        if spawned.is_err() {
            let outcome = Outcome::Failed {
                path: failed_path,
                reason: Failure::Other,
            };
            if let Some(taken) = not_spawned {
                self.history.add(taken, &self.audience);
            }
            let ended = (outcome, Vec::new());
            finish(&self.running, &self.audience, job, ended, Tally::default());
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
                        tally: Tally::default(),
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

    /// Answers `conflict`, a password conflict job `job` holds at, with
    /// `password`. Clients hear it answered with a retry; never the
    /// password.
    pub fn give_password(&self, job: JobId, conflict: &Conflict, password: String) {
        let running = lock(&self.running);
        if let Some(shared) = running.get(&job) {
            shared.control.give_password(conflict, password, || {
                self.audience.tell(&Event::ConflictAnswered {
                    job,
                    answer: Answer::Retry,
                });
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

/// What jobs work with besides the disk.
struct Tools {
    trashes: Trashes,
    archiver: Option<Archiver>,
}

/// Runs `operation`; an undo runs `undo`, the one it took.
fn run(running: &mut Running, operation: &Operation, tools: &Tools, undo: Option<Undo>) -> Ended {
    let trashes = &tools.trashes;
    match operation {
        Operation::Rename { path, name } => one_step::rename(path, name),
        Operation::NewFolder { inside, name } => one_step::new_folder(inside, name),
        Operation::Copy { sources, into } => transfer(running, sources, into, How::Copy),
        Operation::Move { sources, into } => transfer(running, sources, into, How::Move),
        Operation::Delete { paths } => delete(running, paths),
        Operation::ChangeAccess { paths, change } => change_access(running, paths, change),
        Operation::Trash { paths } => trash(running, paths, trashes.home.as_deref()),
        Operation::Restore { items } => restore(running, items, trashes),
        Operation::DeleteFromTrash { items } => delete_from_trash(running, items, trashes),
        Operation::EmptyTrash => empty_trash(running, trashes),
        Operation::Undo { .. } => undo::run(running, undo, trashes),
        Operation::Extract { archive, into, to } => {
            extract(running, tools.archiver.as_ref(), archive, into, *to)
        }
    }
}

/// When job `running` resumes what a crash `left`, takes up its journal
/// and returns what is left to do; `None` when it doesn't resume.
///
/// # Errors
/// [`Cancelled`] when cancelled while what is left is found.
fn resumed(
    running: &mut Running,
    resume: Option<(Left, u64)>,
) -> Option<Result<Operation, Cancelled>> {
    let (left, names) = resume?;
    let rest = still_to_do(&left.operation, &running.shared.control);
    running.resuming(left, names);
    Some(rest)
}

/// What the answers to `running`'s conflicts made of it, and how to undo
/// it, once it ran `operation` and `ended`: what it made taken back, and
/// then what it replaced put back.
fn after(
    running: &mut Running,
    operation: &Operation,
    ended: &Ended,
    trashes: &Trashes,
) -> (Tally, Option<Undo>) {
    let steps: Vec<_> = std::mem::take(&mut running.steps).into_iter().collect();
    let undo = (!running.resumed())
        .then(|| undo_of(operation, &steps, &ended.0, trashes))
        .flatten()
        .map(|undo| undo::putting_back(undo, &running.replaced));
    (running.tally, undo)
}

/// What is left of `operation` after a crash: a move, delete or trash
/// leaves out the sources already gone (moved whole, deleted, or in the
/// trash); a copy does all of it again, taking what it made as done. A
/// source that can't be looked at stays, so the job reports it. Each
/// look is held or stopped through `control` first, so a long list on a
/// slow disk never keeps a pause or cancel waiting.
///
/// # Errors
/// [`Cancelled`] when cancelled.
fn still_to_do(operation: &Operation, control: &Control) -> Result<Operation, Cancelled> {
    let there = |paths: &[PathBuf]| -> Result<Vec<PathBuf>, Cancelled> {
        let mut there = Vec::with_capacity(paths.len());
        for path in paths {
            control.checkpoint()?;
            let gone = matches!(
                path.symlink_metadata(),
                Err(error) if error.kind() == io::ErrorKind::NotFound
            );
            if !gone {
                there.push(path.clone());
            }
        }
        Ok(there)
    };
    Ok(match operation {
        Operation::Move { sources, into } => Operation::Move {
            sources: there(sources)?,
            into: into.clone(),
        },
        Operation::Delete { paths } => Operation::Delete {
            paths: there(paths)?,
        },
        Operation::Trash { paths } => Operation::Trash {
            paths: there(paths)?,
        },
        Operation::Restore { items } => Operation::Restore {
            items: there(items)?,
        },
        Operation::DeleteFromTrash { items } => Operation::DeleteFromTrash {
            items: there(items)?,
        },
        other => other.clone(),
    })
}

/// The path a job that couldn't start failed at.
fn first_path(operation: &Operation) -> PathBuf {
    match operation {
        Operation::Rename { path, .. } | Operation::Extract { archive: path, .. } => path.clone(),
        Operation::NewFolder { inside, name } => inside.join(name),
        Operation::Copy { sources, into } | Operation::Move { sources, into } => {
            sources.first().unwrap_or(into).clone()
        }
        Operation::Delete { paths }
        | Operation::Trash { paths }
        | Operation::ChangeAccess { paths, .. }
        | Operation::Restore { items: paths }
        | Operation::DeleteFromTrash { items: paths } => paths.first().cloned().unwrap_or_default(),
        Operation::EmptyTrash | Operation::Undo { .. } => PathBuf::new(),
    }
}

/// Forgets job `job` and tells every client how it ended, and what the
/// answers to its conflicts made of it.
#[expect(
    clippy::significant_drop_tightening,
    reason = "telling under the lock keeps a pause from being heard after the end"
)]
fn finish(
    registry: &Registry,
    audience: &Audience,
    job: JobId,
    (outcome, changed): Ended,
    tally: Tally,
) {
    let mut running = lock(registry);
    running.remove(&job);
    audience.tell(&Event::JobFinished {
        job,
        outcome,
        changed,
        tally,
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
    fn a_resumed_job_cancelled_while_its_sources_are_looked_at_stops() {
        let folder = TestFolder::new("ops-still-to-do-cancel");
        let delete = Operation::Delete {
            paths: vec![folder.file("file", "f")],
        };
        let control = Control::default();
        control.cancel();

        assert_eq!(still_to_do(&delete, &control), Err(Cancelled));
    }

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

        let left = still_to_do(&delete, &Control::default()).unwrap();

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
