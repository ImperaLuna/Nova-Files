use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use nova_files_api::{Operation, Outcome};

use super::Ended;
use super::journal::{Record, word};
use super::running::Running;
use super::stop::Stop;
use crate::trash::{self, Trashes};
use nova_files_jobs::identity::{self, Identity, same, still};
use nova_files_jobs::mover::Mover;
use nova_files_jobs::parents::make_folders;
use nova_files_jobs::undo::{gone, trash_again};
use nova_files_transfer_core::How;
use nova_files_transfer_core::transfer_to;
use nova_files_trash_core::restore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Undo {
    Trash(Vec<(PathBuf, Identity)>),
    MoveBack(Vec<(PathBuf, PathBuf, Identity)>),
    Restore(Vec<(PathBuf, Identity)>),
    Rename(PathBuf, Identity, OsString),
    RemoveFolder(PathBuf, Identity),
}

pub fn undo_of(
    operation: &Operation,
    steps: &[(PathBuf, PathBuf)],
    outcome: &Outcome,
    trashes: &Trashes,
) -> Option<Undo> {
    let given = || -> Vec<(PathBuf, PathBuf, Identity)> {
        steps
            .iter()
            .filter_map(|(from, to)| Some((to.clone(), from.clone(), identity::of(to)?)))
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
        Operation::Copy { .. }
        | Operation::Restore { .. }
        | Operation::Extract { .. }
        | Operation::Compress { .. } => Undo::Trash(at(given())),
        Operation::Move { .. } => Undo::MoveBack(given()),
        Operation::Trash { .. } => Undo::Restore(
            at(given())
                .into_iter()
                .filter(|(item, _)| trash::trash_of(item, trashes).is_some())
                .collect(),
        ),
        Operation::Rename { path, name } if done && path.file_name() != Some(name.as_os_str()) => {
            let renamed = path.with_file_name(name);
            let entry = identity::of(&renamed)?;
            Undo::Rename(renamed, entry, path.file_name()?.to_os_string())
        }
        Operation::NewFolder { inside, name } if done => {
            let made = inside.join(name);
            let entry = identity::of(&made)?;
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

pub fn run(
    running: &mut Running,
    undo: Option<Undo>,
    trashes: &Trashes,
    mover: &dyn Mover,
) -> Ended {
    let Some(undo) = undo else {
        return gone();
    };
    match undo {
        Undo::Trash(paths) => trash_again(running, paths),
        Undo::Restore(items) => match still(items)
            .into_iter()
            .filter(|item| trash::find(item, trashes).is_some())
            .collect::<Vec<_>>()
        {
            items if items.is_empty() => gone(),
            items => restore(running, &items, trashes, mover),
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
                Ok(()) => nova_files_rename_core::rename(&path, &name),
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

fn move_back(running: &mut Running, pairs: &[(PathBuf, PathBuf)]) -> Ended {
    running.record(Record::Operation(word::UNDO));
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

fn remove_folder(path: &Path) -> Ended {
    match fs::remove_dir(path) {
        Ok(()) => (
            Outcome::Done,
            path.parent().map(Path::to_path_buf).into_iter().collect(),
        ),
        Err(error) => (Stop::at(path, &error).outcome(), Vec::new()),
    }
}

#[cfg(test)]
mod tests;
