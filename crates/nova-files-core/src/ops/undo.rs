//! Undo: the core remembers, for each of the last [`KEPT`] jobs that
//! changed something it can take back, what that is, and a client may
//! ask for it to be taken back as a job of its own.
//!
//! What a job can take back comes from the steps it recorded (see
//! `running`), each the outermost entry it made, moved or trashed from
//! what it was given (inside a folder merged into one already there, the
//! entries that went in): a copy's copies go to the trash; a move's
//! entries go back where they were, into their folders made again when
//! gone; trashed entries are restored; restored entries go back to the
//! trash. A rename is renamed back, and a new folder goes while it is
//! still empty. A delete, a delete from the trash, an empty trash and an
//! undo can't be taken back, nor can a job that resumed after a crash,
//! whose earlier steps this run never saw. What a replace put aside
//! isn't brought back.
//!
//! Each entry to take back is remembered with its device and inode, as
//! it was when the job ended; one that has changed since (removed,
//! replaced, renamed away, or its info file gone from the trash) is left
//! alone, and an undo with nothing left to take back fails as not found.
//! An undo is taken from the history when it starts, so it runs once; it
//! goes back when it fails or is cancelled with something still to take
//! back. Undoing a copy or a restore is a trash, and undoing a trash a
//! restore, journalled as such: after a crash it waits as that job.
//!
//! The history lives in the core's memory: it is lost when the core
//! exits, three minutes after its last client.

use std::collections::{BTreeSet, VecDeque};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use nova_files_api::{Event, Failure, JobId, Operation, Outcome};

use super::audience::Audience;
use super::journal::{Record, word};
use super::make::How;
use super::one_step;
use super::plan::Entry;
use super::restore::{make_folders, restore};
use super::running::Running;
use super::stop::Stop;
use super::transfer::transfer_to;
use super::trash::trash;
use super::{Ended, lock};
use crate::trash::{self, Trashes};

/// How many jobs the history keeps.
pub const KEPT: usize = 50;

/// Which entry is at a path: its device and inode, and when it was
/// made where the filesystem says, as an inode freed by a delete may be
/// given to the next file made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    entry: Entry,
    born: Option<SystemTime>,
}

/// What undoing one job does, each entry with which it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Undo {
    /// Send these to the trash.
    Trash(Vec<(PathBuf, Identity)>),
    /// Move each from the first path back to the second.
    MoveBack(Vec<(PathBuf, PathBuf, Identity)>),
    /// Restore these entries from the trash.
    Restore(Vec<(PathBuf, Identity)>),
    /// Give the entry at the path its old name back.
    Rename(PathBuf, Identity, OsString),
    /// Remove this folder, while empty.
    RemoveFolder(PathBuf, Identity),
}

/// A job that can be undone: its id, what it did, and how to undo it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Undoable {
    pub job: JobId,
    pub operation: Operation,
    pub undo: Undo,
}

/// The jobs that can be undone, oldest first.
#[derive(Debug, Default)]
pub struct History {
    jobs: Mutex<VecDeque<Undoable>>,
}

impl History {
    /// `undoable` can be undone: every client hears so, and of the job it
    /// pushes out. One put back after its undo failed goes where its age
    /// puts it.
    pub fn add(&self, undoable: Undoable, audience: &Audience) {
        let mut jobs = lock(&self.jobs);
        audience.tell(&Event::Undoable {
            job: undoable.job,
            operation: undoable.operation.clone(),
        });
        // Job ids are never the same, so it is never found.
        let (Ok(at) | Err(at)) = jobs.binary_search_by_key(&undoable.job, |kept| kept.job);
        jobs.insert(at, undoable);
        // One at a time: at most one too many.
        if jobs.len() > KEPT
            && let Some(gone) = jobs.pop_front()
        {
            audience.tell(&Event::UndoGone(gone.job));
        }
        drop(jobs);
    }

    /// Takes job `job` out of the history, telling every client.
    pub fn take(&self, job: JobId, audience: &Audience) -> Option<Undoable> {
        let mut jobs = lock(&self.jobs);
        let at = jobs.iter().position(|kept| kept.job == job)?;
        let taken = jobs.remove(at)?;
        audience.tell(&Event::UndoGone(job));
        drop(jobs);
        Some(taken)
    }

    /// Calls `then` with an [`Event::Undoable`] for each job that can be
    /// undone, oldest first, with the history held, so no job is added
    /// unheard meanwhile.
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

/// What undoing `operation` does, which made `steps` (from, to) as it
/// ran and ended `outcome`; `None` when there is nothing to take back. A
/// trashed entry counts only in a trash `trashes` can restore from.
pub fn undo_of(
    operation: &Operation,
    steps: &[(PathBuf, PathBuf)],
    outcome: &Outcome,
    trashes: &Trashes,
) -> Option<Undo> {
    let given = || -> Vec<(PathBuf, PathBuf, Identity)> {
        steps
            .iter()
            .filter_map(|(from, to)| Some((to.clone(), from.clone(), entry(to)?)))
            .collect()
    };
    let at = |moved: Vec<(PathBuf, PathBuf, Identity)>| -> Vec<(PathBuf, Identity)> {
        moved
            .into_iter()
            .map(|(to, _, entry)| (to, entry))
            .collect()
    };
    let done = *outcome == Outcome::Done;
    let undo = match operation {
        Operation::Copy { .. } | Operation::Restore { .. } => Undo::Trash(at(given())),
        Operation::Move { .. } => Undo::MoveBack(given()),
        Operation::Trash { .. } => Undo::Restore(
            at(given())
                .into_iter()
                .filter(|(item, _)| trash::trash_of(item, trashes).is_some())
                .collect(),
        ),
        Operation::Rename { path, name } if done && path.file_name() != Some(name.as_os_str()) => {
            let renamed = path.with_file_name(name);
            let entry = entry(&renamed)?;
            Undo::Rename(renamed, entry, path.file_name()?.to_os_string())
        }
        Operation::NewFolder { inside, name } if done => {
            let made = inside.join(name);
            let entry = entry(&made)?;
            Undo::RemoveFolder(made, entry)
        }
        _ => return None,
    };
    let empty = match &undo {
        Undo::Trash(paths) | Undo::Restore(paths) => paths.is_empty(),
        Undo::MoveBack(pairs) => pairs.is_empty(),
        Undo::Rename(..) | Undo::RemoveFolder(..) => false,
    };
    (!empty).then_some(undo)
}

/// Takes back what `undo` says, as job `running`; `None` when the job
/// to undo can't be.
pub fn run(running: &mut Running, undo: Option<Undo>, trashes: &Trashes) -> Ended {
    let gone = || {
        let outcome = Outcome::Failed {
            path: PathBuf::new(),
            reason: Failure::NotFound,
        };
        (outcome, Vec::new())
    };
    let Some(undo) = undo else {
        return gone();
    };
    let same = |path: &Path, was: Identity| entry(path) == Some(was);
    let still = |paths: Vec<(PathBuf, Identity)>| -> Vec<PathBuf> {
        paths
            .into_iter()
            .filter(|(path, was)| same(path, *was))
            .map(|(path, _)| path)
            .collect()
    };
    match undo {
        Undo::Trash(paths) => match still(paths) {
            paths if paths.is_empty() => gone(),
            paths => trash(running, &paths, trashes.home.as_deref()),
        },
        // One whose info file went can't be restored; the rest can.
        Undo::Restore(items) => match still(items)
            .into_iter()
            .filter(|item| trash::find(item, trashes).is_some())
            .collect::<Vec<_>>()
        {
            items if items.is_empty() => gone(),
            items => restore(running, &items, trashes),
        },
        Undo::MoveBack(pairs) => {
            let pairs: Vec<_> = pairs
                .into_iter()
                .filter(|(to, _, was)| same(to, *was))
                .map(|(to, from, _)| (to, from))
                .collect();
            if pairs.is_empty() {
                return gone();
            }
            move_back(running, &pairs)
        }
        Undo::Rename(path, was, name) if same(&path, was) => {
            match running.shared.control.checkpoint().map_err(Stop::from) {
                Ok(()) => one_step::rename(&path, &name),
                Err(stop) => (stop.outcome(), Vec::new()),
            }
        }
        Undo::RemoveFolder(path, was) if same(&path, was) => {
            match running.shared.control.checkpoint().map_err(Stop::from) {
                Ok(()) => remove_folder(&path),
                Err(stop) => (stop.outcome(), Vec::new()),
            }
        }
        Undo::Rename(..) | Undo::RemoveFolder(..) => gone(),
    }
}

/// Moves each entry of `pairs` from its first path back to its second.
/// Its journal says only that it undoes: after a crash its leftovers are
/// cleaned up, and it isn't resumed.
fn move_back(running: &mut Running, pairs: &[(PathBuf, PathBuf)]) -> Ended {
    running.record(Record::Operation(word::UNDO));
    // The folders they were in, made again when gone since, as by a
    // merge that emptied them.
    let mut made = BTreeSet::new();
    let mut targets: Vec<_> = pairs.iter().map(|(_, from)| from.as_path()).collect();
    targets.sort_by_key(|target| target.components().count());
    if let Err(stop) = make_folders(running, &targets, &mut made) {
        return (stop.outcome(), made.into_iter().collect());
    }
    let (result, touched) = transfer_to(running, pairs, How::Move);
    let shown = pairs
        .iter()
        .flat_map(|(to, from)| [to.parent(), from.parent()])
        .flatten()
        .filter(|folder| touched.contains(*folder))
        .map(Path::to_path_buf)
        .chain(made)
        .collect::<BTreeSet<_>>();
    (
        result.map_or_else(Stop::outcome, |()| Outcome::Done),
        shown.into_iter().collect(),
    )
}

/// Removes the empty folder at `path`.
fn remove_folder(path: &Path) -> Ended {
    match fs::remove_dir(path) {
        Ok(()) => (
            Outcome::Done,
            path.parent().map(Path::to_path_buf).into_iter().collect(),
        ),
        Err(error) => (Stop::at(path, &error).outcome(), Vec::new()),
    }
}

/// Which entry is at `path` now, links not followed.
fn entry(path: &Path) -> Option<Identity> {
    let there = path.symlink_metadata().ok()?;
    Some(Identity {
        entry: Entry::of(&there),
        born: there.created().ok(),
    })
}

#[cfg(test)]
mod tests;
