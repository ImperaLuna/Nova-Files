mod access;
mod ahead;
mod attrs;
mod audience;
mod clash;
mod compress;
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
use crate::inside;
use crate::jobs::Client;
use crate::trash::Trashes;
use access::change_access;
use control::{Cancelled, Control};
use delete::delete;
use extract::{Taking, extract};
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
const DROP_THREAD: &str = "nova-files-drop";
const RECOVER_THREAD: &str = "nova-files-recover";
const FORGET_THREAD: &str = "nova-files-forget";

pub type Ended = (Outcome, Vec<PathBuf>);

type Registry = Arc<Mutex<HashMap<JobId, Arc<Job>>>>;
type Interrupted = Arc<Mutex<HashMap<JobId, Left>>>;

pub struct Jobs {
    next: AtomicU64,
    audience: Audience,
    running: Registry,
    journals: Option<PathBuf>,
    trashes: Trashes,
    interrupted: Interrupted,
    wake: Connector,
    history: Arc<History>,
    archiver: Option<Archiver>,
}

impl Jobs {
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
        client: &Client,
        tag: JobTag,
        operation: Operation,
    ) -> Result<(), ClientGone> {
        self.start_for(client.events().as_ref(), tag, operation)
    }

    fn start_for(
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

    pub fn start_drop(
        self: &Arc<Self>,
        events: &Arc<dyn EventSink>,
        tag: JobTag,
        sources: Vec<PathBuf>,
        into: PathBuf,
    ) {
        let jobs = Arc::clone(self);
        let asked = Arc::new(Mutex::new(Some((sources, into))));
        let taken = Arc::clone(&asked);
        let thread_events = Arc::clone(events);
        let spawned = thread::Builder::new()
            .name(DROP_THREAD.to_owned())
            .spawn(move || {
                let asked = lock(&taken).take();
                if let Some((sources, into)) = asked {
                    let operation = dropped::dropped(sources, into);
                    jobs.start_for(thread_events.as_ref(), tag, operation).ok();
                }
            });
        let unasked = spawned.is_err().then(|| lock(&asked).take()).flatten();
        if let Some((sources, into)) = unasked {
            let copy = Operation::Copy { sources, into };
            self.start_for(events.as_ref(), tag, copy).ok();
        }
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
        let failed_path = first_path(&operation);
        let reporter = Reporter::new(
            self.audience.clone(),
            Arc::clone(&self.running),
            job,
            Arc::clone(&shared),
        );
        let mut running = Running::new(job, shared, reporter, self.journals.clone());
        running.home_trash.clone_from(&self.trashes.home);
        let resume = left.map(|left| (left, self.next.fetch_add(1, Ordering::Relaxed)));
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

    pub fn cancel(&self, job: JobId) {
        if let Some(shared) = lock(&self.running).get(&job) {
            shared.control.cancel();
        }
    }
}

fn first_id() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_micros()).unwrap_or(u64::MAX)
        })
}

struct Tools {
    trashes: Trashes,
    archiver: Option<Archiver>,
}

fn run(running: &mut Running, operation: &Operation, tools: &Tools, undo: Option<Undo>) -> Ended {
    let trashes = &tools.trashes;
    match operation {
        Operation::Rename { path, name } => one_step::rename(path, name),
        Operation::NewFolder { inside, name } => one_step::new_folder(inside, name),
        Operation::Copy { sources, into } => match copied_out(sources) {
            Some((archive, picked)) => {
                let taking = Taking::Picked(&picked);
                extract(running, tools.archiver.as_ref(), &archive, into, taking)
            }
            None => transfer(running, sources, into, How::Copy),
        },
        Operation::Move { sources, into } => transfer(running, sources, into, How::Move),
        Operation::Delete { paths } => delete(running, paths),
        Operation::ChangeAccess { paths, change } => change_access(running, paths, change),
        Operation::Trash { paths } => trash(running, paths, trashes.home.as_deref()),
        Operation::Restore { items } => restore(running, items, trashes),
        Operation::DeleteFromTrash { items } => delete_from_trash(running, items, trashes),
        Operation::EmptyTrash => empty_trash(running, trashes),
        Operation::Undo { .. } => undo::run(running, undo, trashes),
        Operation::Extract { archive, into, to } => extract(
            running,
            tools.archiver.as_ref(),
            archive,
            into,
            Taking::All(*to),
        ),
        Operation::Compress {
            sources,
            into,
            name,
            packing,
            level,
            locked,
        } => {
            let asked = compress::Asked {
                sources,
                into,
                name,
                packing: *packing,
                level: *level,
                locked: *locked,
            };
            compress::compress(running, tools.archiver.as_ref(), &asked)
        }
    }
}

fn copied_out(sources: &[PathBuf]) -> Option<(PathBuf, Vec<PathBuf>)> {
    let mut found = sources.iter().map(|source| {
        source
            .symlink_metadata()
            .is_err()
            .then(|| inside::within(source))
            .flatten()
    });
    let first = found.next()??;
    let mut picked = vec![first.inner];
    for within in found {
        let within = within.filter(|within| within.archive == first.archive)?;
        picked.push(within.inner);
    }
    picked
        .iter()
        .all(|inner| inner.file_name().is_some())
        .then_some((first.archive, picked))
}

fn resumed(
    running: &mut Running,
    resume: Option<(Left, u64)>,
) -> Option<Result<Operation, Cancelled>> {
    let (left, names) = resume?;
    let rest = still_to_do(&left.operation, &running.shared.control);
    running.resuming(left, names);
    Some(rest)
}

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

fn first_path(operation: &Operation) -> PathBuf {
    match operation {
        Operation::Rename { path, .. } | Operation::Extract { archive: path, .. } => path.clone(),
        Operation::NewFolder { inside, name } => inside.join(name),
        Operation::Copy { sources, into }
        | Operation::Move { sources, into }
        | Operation::Compress { sources, into, .. } => sources.first().unwrap_or(into).clone(),
        Operation::Delete { paths }
        | Operation::Trash { paths }
        | Operation::ChangeAccess { paths, .. }
        | Operation::Restore { items: paths }
        | Operation::DeleteFromTrash { items: paths } => paths.first().cloned().unwrap_or_default(),
        Operation::EmptyTrash | Operation::Undo { .. } => PathBuf::new(),
    }
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
