use std::path::PathBuf;

use nova_files_api::{JobId, Operation};
use nova_files_jobs::control::{Cancelled, Control};
use nova_files_jobs::identity::{self, Identity, still};
use nova_files_jobs::journal::word;
use nova_files_jobs::mover::Mover;
use nova_files_jobs::rest::still_there;
use nova_files_jobs::running::Running;
use nova_files_jobs::undo::{gone, trash_again};
use nova_files_jobs::{Ended, JobKind};
use nova_files_trash_api::{
    DeleteFromTrashJob, EmptyTrashJob, RestoreJob, TrashJob, delete_from_trash as deleting,
    empty_trash as emptying, restore as restoring, trash as trashing,
};

use crate::{Trashes, delete_from_trash, empty_trash, find, restore, trash, trash_of};

pub struct TrashJobs<M> {
    pub trashes: Trashes,
    pub files: M,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Undo {
    Trash(Vec<(PathBuf, Identity)>),
    Restore(Vec<(PathBuf, Identity)>),
}

impl<M: Mover + 'static> JobKind for TrashJobs<M> {
    type Op = Operation;
    type Undo = Undo;

    fn claims(&self, operation: &Operation) -> bool {
        asked(operation).is_some() || matches!(operation, Operation::Undo { .. })
    }

    fn run(&self, running: &mut Running, operation: &Operation, undo: Option<&Undo>) -> Ended {
        let trashes = &self.trashes;
        match asked(operation) {
            Some(Asked::Trash(paths)) => trash(running, paths, trashes.home.as_deref()),
            Some(Asked::Restore(items)) => restore(running, items, trashes, &self.files),
            Some(Asked::DeleteFromTrash(items)) => {
                delete_from_trash(running, items, trashes, &self.files)
            }
            Some(Asked::Empty) => empty_trash(running, trashes, &self.files),
            None => run_undo(running, undo.cloned(), trashes, &self.files),
        }
    }

    fn rest(&self, operation: &Operation, control: &Control) -> Result<Operation, Cancelled> {
        let there = |paths: &[PathBuf]| still_there(paths, control);
        Ok(match asked(operation) {
            Some(Asked::Trash(paths)) => trashing(there(paths)?),
            Some(Asked::Restore(items)) => restoring(there(items)?),
            Some(Asked::DeleteFromTrash(items)) => deleting(there(items)?),
            Some(Asked::Empty) | None => operation.clone(),
        })
    }

    fn undo(&self, running: &mut Running, operation: &Operation, _: &Ended) -> Option<Undo> {
        let steps: Vec<_> = std::mem::take(&mut running.steps).into_iter().collect();
        undo_of(operation, &steps, &self.trashes)
    }

    fn undoes(&self, operation: &Operation) -> Option<JobId> {
        match operation {
            Operation::Undo { job } => Some(*job),
            Operation::Feature(_) => None,
        }
    }

    fn first_path(&self, operation: &Operation) -> PathBuf {
        match asked(operation) {
            Some(Asked::Trash(paths) | Asked::Restore(paths) | Asked::DeleteFromTrash(paths)) => {
                paths.first().cloned().unwrap_or_default()
            }
            Some(Asked::Empty) | None => PathBuf::new(),
        }
    }

    fn resume(&self, name: &str, into: Option<PathBuf>, paths: Vec<PathBuf>) -> Option<Operation> {
        if into.is_some() {
            return None;
        }
        match name {
            word::TRASH => Some(trashing(paths)),
            word::RESTORE => Some(restoring(paths)),
            word::DELETE_FROM_TRASH => Some(deleting(paths)),
            word::EMPTY_TRASH => Some(emptying()),
            _ => None,
        }
    }
}

enum Asked<'a> {
    Trash(&'a [PathBuf]),
    Restore(&'a [PathBuf]),
    DeleteFromTrash(&'a [PathBuf]),
    Empty,
}

fn asked(operation: &Operation) -> Option<Asked<'_>> {
    if let Some(job) = operation.get::<TrashJob>() {
        return Some(Asked::Trash(&job.paths));
    }
    if let Some(job) = operation.get::<RestoreJob>() {
        return Some(Asked::Restore(&job.items));
    }
    if let Some(job) = operation.get::<DeleteFromTrashJob>() {
        return Some(Asked::DeleteFromTrash(&job.items));
    }
    operation.get::<EmptyTrashJob>().map(|_| Asked::Empty)
}

fn undo_of(operation: &Operation, steps: &[(PathBuf, PathBuf)], trashes: &Trashes) -> Option<Undo> {
    let at = steps
        .iter()
        .filter_map(|(_, to)| Some((to.clone(), identity::of(to)?)));
    let undo = match asked(operation)? {
        Asked::Restore(_) => Undo::Trash(at.collect()),
        Asked::Trash(_) => Undo::Restore(
            at.filter(|(item, _)| trash_of(item, trashes).is_some())
                .collect(),
        ),
        Asked::DeleteFromTrash(_) | Asked::Empty => return None,
    };
    let (Undo::Trash(paths) | Undo::Restore(paths)) = &undo;
    (!paths.is_empty()).then_some(undo)
}

fn run_undo(
    running: &mut Running,
    undo: Option<Undo>,
    trashes: &Trashes,
    mover: &dyn Mover,
) -> Ended {
    match undo {
        None => gone(),
        Some(Undo::Trash(paths)) => trash_again(running, paths),
        Some(Undo::Restore(items)) => match still(items)
            .into_iter()
            .filter(|item| find(item, trashes).is_some())
            .collect::<Vec<_>>()
        {
            items if items.is_empty() => gone(),
            items => restore(running, &items, trashes, mover),
        },
    }
}

#[cfg(test)]
mod tests;
