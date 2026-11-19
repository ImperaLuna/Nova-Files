use std::path::PathBuf;

use nova_files_api::Operation;

use super::Ended;
use super::running::Running;
use crate::backend::trash::{self, Trashes};
use nova_files_jobs::identity::{self, Identity, still};
use nova_files_jobs::mover::Mover;
use nova_files_jobs::undo::{gone, trash_again};
use nova_files_trash_core::restore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Undo {
    Trash(Vec<(PathBuf, Identity)>),
    Restore(Vec<(PathBuf, Identity)>),
}

pub fn undo_of(
    operation: &Operation,
    steps: &[(PathBuf, PathBuf)],
    trashes: &Trashes,
) -> Option<Undo> {
    let at = steps
        .iter()
        .filter_map(|(_, to)| Some((to.clone(), identity::of(to)?)));
    let undo = match operation {
        Operation::Copy { .. } | Operation::Restore { .. } => Undo::Trash(at.collect()),
        Operation::Trash { .. } => Undo::Restore(
            at.filter(|(item, _)| trash::trash_of(item, trashes).is_some())
                .collect(),
        ),
        _ => return None,
    };
    let (Undo::Trash(paths) | Undo::Restore(paths)) = &undo;
    (!paths.is_empty()).then_some(undo)
}

pub fn run(
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
            .filter(|item| trash::find(item, trashes).is_some())
            .collect::<Vec<_>>()
        {
            items if items.is_empty() => gone(),
            items => restore(running, &items, trashes, mover),
        },
    }
}

#[cfg(test)]
mod tests;
