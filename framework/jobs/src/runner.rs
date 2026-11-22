use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use nova_files_api::{
    Answer, Carried, ClientGone, ClientId, Conflict, Connector, Event, EventSink, Failure, JobId,
    JobTag, Operation, Outcome, Scope, Tally,
};
use nova_files_messages::Message;

use crate::audience::Audience;
use crate::control::{Cancelled, Control};
use crate::history::{History, Undoable};
use crate::job::Job;
use crate::left::{self, Left};
use crate::progress::Reporter;
use crate::running::Running;
use crate::set_aside::SetAside;
use crate::{Registry, journal, lock, undo};

const JOB_THREAD: &str = "nova-files-job";
const RECOVER_THREAD: &str = "nova-files-recover";
const FORGET_THREAD: &str = "nova-files-forget";

pub type Ended = (Outcome, Vec<PathBuf>);

pub trait JobKind: Send + Sync + 'static {
    type Op: Message + Clone;
    type Undo: Clone + Send + 'static;

    fn claims(&self, _: &Self::Op) -> bool {
        true
    }

    fn run(&self, running: &mut Running, op: &Self::Op, undo: Option<&Self::Undo>) -> Ended;

    fn rest(&self, op: &Self::Op, _: &Control) -> Result<Self::Op, Cancelled> {
        Ok(op.clone())
    }

    fn undo(&self, _: &mut Running, _: &Self::Op, _: &Ended) -> Option<Self::Undo> {
        None
    }

    fn undoes(&self, _: &Self::Op) -> Option<JobId> {
        None
    }

    fn first_path(&self, op: &Self::Op) -> PathBuf;

    fn resume(&self, _: &str, _: Option<PathBuf>, _: Vec<PathBuf>) -> Option<Self::Op> {
        None
    }
}

type Interrupted = Arc<Mutex<HashMap<JobId, Left>>>;

pub struct Jobs<K: JobKind<Op = Operation>> {
    next: AtomicU64,
    audience: Audience,
    running: Registry,
    journals: Option<PathBuf>,
    kind: Arc<K>,
    set_aside: Arc<dyn SetAside>,
    interrupted: Interrupted,
    wake: Connector,
    history: Arc<History<K::Undo>>,
}

impl<K: JobKind<Op = Operation>> Jobs<K> {
    pub fn new(
        state: Option<&Path>,
        kind: Arc<K>,
        set_aside: Arc<dyn SetAside>,
        wake: Connector,
    ) -> Self {
        let journals = state.map(|state| state.join(journal::FOLDER));
        let jobs = Self {
            next: AtomicU64::new(first_id()),
            audience: Audience::default(),
            running: Registry::default(),
            journals,
            kind,
            set_aside,
            interrupted: Interrupted::default(),
            wake,
            history: Arc::default(),
        };
        if let Some(folder) = jobs.journals.clone() {
            jobs.recover(folder);
        }
        jobs
    }

    fn recover(&self, folder: PathBuf) {
        let (running, interrupted, audience, kind) = (
            Arc::clone(&self.running),
            Arc::clone(&self.interrupted),
            self.audience.clone(),
            Arc::clone(&self.kind),
        );
        let spawned = thread::Builder::new()
            .name(RECOVER_THREAD.to_owned())
            .spawn(move || {
                let found = left::recover(&folder, &|word, into, sources| {
                    kind.resume(word, into, sources)
                });
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

    pub const fn audience(&self) -> &Audience {
        &self.audience
    }

    pub fn watch(&self, client: ClientId, events: &Arc<dyn EventSink>) {
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

    pub fn busy(&self) -> bool {
        !lock(&self.running).is_empty()
    }

    pub fn start(
        &self,
        events: &dyn EventSink,
        tag: JobTag,
        operation: Operation,
    ) -> Result<(), ClientGone> {
        let job = JobId(self.next.fetch_add(1, Ordering::Relaxed));
        let told = events.deliver(Event::JobStarted {
            tag,
            job,
            operation: operation.clone(),
        });
        self.spawn(lock(&self.running), job, operation, None);
        told
    }

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
        let failed_path = self.kind.first_path(&operation);
        let reporter = Reporter::new(
            self.audience.clone(),
            Arc::clone(&self.running),
            job,
            Arc::clone(&shared),
        );
        let mut running = Running::new(job, shared, reporter, self.journals.clone());
        running.set_aside_with(self.set_aside.for_job());
        let resume = left.map(|left| (left, self.next.fetch_add(1, Ordering::Relaxed)));
        let taken = self
            .kind
            .undoes(&operation)
            .and_then(|undone| self.history.take(undone, &self.audience));
        let not_spawned = taken.clone();
        let (registry, audience, wake, history, kind) = (
            Arc::clone(&self.running),
            self.audience.clone(),
            self.wake.clone(),
            Arc::clone(&self.history),
            Arc::clone(&self.kind),
        );
        let spawned = thread::Builder::new()
            .name(JOB_THREAD.to_owned())
            .spawn(move || {
                let undo = taken.as_ref().map(|taken| &taken.undo);
                let ended = match resumed(kind.as_ref(), &mut running, resume) {
                    None => kind.run(&mut running, &operation, undo),
                    Some(Ok(rest)) => kind.run(&mut running, &rest, undo),
                    Some(Err(Cancelled)) => (Outcome::Cancelled, Vec::new()),
                };
                let undoable = if running.resumed() {
                    None
                } else {
                    kind.undo(&mut running, &operation, &ended)
                };
                let tally = running.tally;
                running.finish();
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
                if let Some(taken) = taken.filter(|_| !undo::spent(&ended)) {
                    history.add(taken, &audience);
                }
                finish(&registry, &audience, job, ended, tally);
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

    pub fn pause(&self, job: JobId) {
        let running = lock(&self.running);
        if running
            .get(&job)
            .is_some_and(|shared| shared.control.pause())
        {
            self.audience.tell(&Event::JobPaused(job));
        }
    }

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
                    let operation = left.operation.clone();
                    lock(&interrupted).insert(job, left);
                    audience.tell(&Event::JobInterrupted { job, operation });
                }
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

    pub fn answer(&self, job: JobId, conflict: &Conflict, answer: Answer, scope: Scope) {
        let running = lock(&self.running);
        if let Some(shared) = running.get(&job) {
            shared.control.answer(conflict, answer, scope, || {
                self.audience.tell(&Event::ConflictAnswered { job, answer });
            });
        }
        drop(running);
    }

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

    pub fn reply(&self, job: JobId, conflict: &Conflict, reply: Carried) {
        let running = lock(&self.running);
        if let Some(shared) = running.get(&job) {
            shared.control.reply(conflict, reply, || {
                self.audience.tell(&Event::ConflictAnswered {
                    job,
                    answer: Answer::Retry,
                });
            });
        }
        drop(running);
    }

    pub fn cancel(&self, job: JobId) {
        if let Some(shared) = lock(&self.running).get(&job) {
            shared.control.cancel();
        }
    }
}

fn resumed<K: JobKind<Op = Operation>>(
    kind: &K,
    running: &mut Running,
    resume: Option<(Left, u64)>,
) -> Option<Result<Operation, Cancelled>> {
    let (left, names) = resume?;
    let rest = kind.rest(&left.operation, &running.shared.control);
    running.resuming(left, names);
    Some(rest)
}

fn first_id() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_micros()).unwrap_or(u64::MAX)
        })
}

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

#[cfg(test)]
mod tests;
