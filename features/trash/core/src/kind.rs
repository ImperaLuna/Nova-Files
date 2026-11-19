use std::path::PathBuf;

use nova_files_api::{JobId, Operation};
use nova_files_jobs::control::{Cancelled, Control};
use nova_files_jobs::identity::{self, Identity, still};
use nova_files_jobs::mover::Mover;
use nova_files_jobs::rest::still_there;
use nova_files_jobs::running::Running;
use nova_files_jobs::undo::{gone, trash_again};
use nova_files_jobs::{Ended, JobKind};

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
        matches!(
            operation,
            Operation::Trash { .. }
                | Operation::Restore { .. }
                | Operation::DeleteFromTrash { .. }
                | Operation::EmptyTrash
                | Operation::Undo { .. }
        )
    }

    fn run(&self, running: &mut Running, operation: &Operation, undo: Option<&Undo>) -> Ended {
        let trashes = &self.trashes;
        match operation {
            Operation::Trash { paths } => trash(running, paths, trashes.home.as_deref()),
            Operation::Restore { items } => restore(running, items, trashes, &self.files),
            Operation::DeleteFromTrash { items } => {
                delete_from_trash(running, items, trashes, &self.files)
            }
            Operation::EmptyTrash => empty_trash(running, trashes, &self.files),
            _ => run_undo(running, undo.cloned(), trashes, &self.files),
        }
    }

    fn rest(&self, operation: &Operation, control: &Control) -> Result<Operation, Cancelled> {
        let there = |paths: &[PathBuf]| still_there(paths, control);
        Ok(match operation {
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

    fn undo(&self, running: &mut Running, operation: &Operation, _: &Ended) -> Option<Undo> {
        let steps: Vec<_> = std::mem::take(&mut running.steps).into_iter().collect();
        undo_of(operation, &steps, &self.trashes)
    }

    fn undoes(&self, operation: &Operation) -> Option<JobId> {
        match operation {
            Operation::Undo { job } => Some(*job),
            _ => None,
        }
    }

    fn first_path(&self, operation: &Operation) -> PathBuf {
        match operation {
            Operation::Trash { paths }
            | Operation::Restore { items: paths }
            | Operation::DeleteFromTrash { items: paths } => {
                paths.first().cloned().unwrap_or_default()
            }
            _ => PathBuf::new(),
        }
    }
}

fn undo_of(operation: &Operation, steps: &[(PathBuf, PathBuf)], trashes: &Trashes) -> Option<Undo> {
    let at = steps
        .iter()
        .filter_map(|(_, to)| Some((to.clone(), identity::of(to)?)));
    let undo = match operation {
        Operation::Restore { .. } => Undo::Trash(at.collect()),
        Operation::Trash { .. } => Undo::Restore(
            at.filter(|(item, _)| trash_of(item, trashes).is_some())
                .collect(),
        ),
        _ => return None,
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
