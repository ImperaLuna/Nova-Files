use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use nova_files_api::{Failure, Operation, Outcome};

use super::Ended;
use super::journal::{Record, word};
use super::make::How;
use super::one_step;
use super::plan::Entry;
use super::restore::{make_folders, put_back, restore};
use super::running::Running;
use super::stop::Stop;
use super::transfer::transfer_to;
use super::trash::trash;
use crate::trash::{self, Trashes};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    entry: Entry,
    born: Option<SystemTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Undo {
    Trash(Vec<(PathBuf, Identity)>),
    MoveBack(Vec<(PathBuf, PathBuf, Identity)>),
    Restore(Vec<(PathBuf, Identity)>),
    Rename(PathBuf, Identity, OsString),
    RemoveFolder(PathBuf, Identity),
    ThenPutBack(Box<Self>, Vec<(PathBuf, Identity)>),
}

pub fn putting_back(undo: Undo, replaced: &[(PathBuf, PathBuf)]) -> Undo {
    let items: Vec<_> = replaced
        .iter()
        .filter_map(|(_, item)| Some((item.clone(), entry(item)?)))
        .collect();
    if items.is_empty() {
        undo
    } else {
        Undo::ThenPutBack(Box::new(undo), items)
    }
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
        Undo::Rename(..) | Undo::RemoveFolder(..) | Undo::ThenPutBack(..) => false,
    };
    (!empty).then_some(undo)
}

pub fn run(running: &mut Running, undo: Option<Undo>, trashes: &Trashes) -> Ended {
    let Some(Undo::ThenPutBack(first, replaced)) = undo else {
        return run_one(running, undo, trashes);
    };
    let (outcome, mut changed) = run_one(running, Some(*first), trashes);
    let free = outcome == Outcome::Done || is_gone(&outcome);
    if !free {
        return (outcome, changed);
    }
    let items: Vec<_> = still(replaced)
        .into_iter()
        .filter(|item| {
            trash::find(item, trashes)
                .is_some_and(|(_, found)| found.original.symlink_metadata().is_err())
        })
        .collect();
    if items.is_empty() {
        return (outcome, changed);
    }
    let (put, more) = put_back(running, &items, trashes);
    changed.extend(more);
    changed.sort();
    changed.dedup();
    (put, changed)
}

fn is_gone(outcome: &Outcome) -> bool {
    matches!(outcome, Outcome::Failed { path, reason: Failure::NotFound } if path.as_os_str().is_empty())
}

fn still(paths: Vec<(PathBuf, Identity)>) -> Vec<PathBuf> {
    paths
        .into_iter()
        .filter(|(path, was)| entry(path) == Some(*was))
        .map(|(path, _)| path)
        .collect()
}

fn run_one(running: &mut Running, undo: Option<Undo>, trashes: &Trashes) -> Ended {
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
    match undo {
        Undo::Trash(paths) => match still(paths) {
            paths if paths.is_empty() => gone(),
            paths => trash(running, &paths, trashes.home.as_deref()),
        },
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
        Undo::Rename(..) | Undo::RemoveFolder(..) | Undo::ThenPutBack(..) => gone(),
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

fn entry(path: &Path) -> Option<Identity> {
    let there = path.symlink_metadata().ok()?;
    Some(Identity {
        entry: Entry::of(&there),
        born: there.created().ok(),
    })
}

#[cfg(test)]
mod tests;
