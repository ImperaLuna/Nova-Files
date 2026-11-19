mod undo;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use nova_files_api::{Connector, EventSink, JobId, JobTag, Operation};
use nova_files_jobs::control::{Cancelled, Control};
use nova_files_jobs::kinds::Kinds;
use nova_files_jobs::mover::{Moved, Mover};
use nova_files_jobs::rest::still_there;
use nova_files_jobs::running::{self, Running};
use nova_files_jobs::{Ended, lock};
use nova_files_properties_core::change_access;

#[cfg(test)]
pub use nova_files_jobs::test_folder::TestFolder;

use crate::inside;
use crate::trash::Trashes;
use nova_files_archives_core::{Archiver, Archives, Taking, extract};
use nova_files_delete_core::{Deletes, delete_paths};
use nova_files_rename_core::Renames;
use nova_files_transfer_core::{How, Transfers, transfer, transfer_to};
use nova_files_trash_core::{Trasher, delete_from_trash, empty_trash, restore, trash};
use undo::{Undo, undo_of};

const DROP_THREAD: &str = "nova-files-drop";

pub type Jobs = nova_files_jobs::Jobs<Kinds>;

pub fn jobs(
    state: Option<&Path>,
    trashes: Trashes,
    wake: Connector,
    archiver: Option<Archiver>,
) -> Jobs {
    let set_aside = Arc::new(Trasher::new(trashes.clone(), Arc::new(Moves)));
    Jobs::new(state, Arc::new(kinds(trashes, archiver)), set_aside, wake)
}

fn kinds(trashes: Trashes, archiver: Option<Archiver>) -> Kinds {
    let files = Files {
        trashes,
        archiver: archiver.clone(),
        mover: Moves,
    };
    Kinds::default()
        .with(files)
        .with(Archives {
            archiver,
            files: Moves,
        })
        .with(Deletes)
        .with(Renames)
        .with(Transfers)
}

pub fn start_drop(
    jobs: &Arc<Jobs>,
    events: &Arc<dyn EventSink>,
    tag: JobTag,
    sources: Vec<PathBuf>,
    into: PathBuf,
) {
    let thread_jobs = Arc::clone(jobs);
    let asked = Arc::new(Mutex::new(Some((sources, into))));
    let taken = Arc::clone(&asked);
    let thread_events = Arc::clone(events);
    let spawned = thread::Builder::new()
        .name(DROP_THREAD.to_owned())
        .spawn(move || {
            let asked = lock(&taken).take();
            if let Some((sources, into)) = asked {
                let operation = nova_files_transfer_core::dropped(sources, into);
                thread_jobs
                    .start(thread_events.as_ref(), tag, operation)
                    .ok();
            }
        });
    let unasked = spawned.is_err().then(|| lock(&asked).take()).flatten();
    if let Some((sources, into)) = unasked {
        let copy = Operation::Copy { sources, into };
        jobs.start(events.as_ref(), tag, copy).ok();
    }
}

pub struct Files {
    trashes: Trashes,
    archiver: Option<Archiver>,
    mover: Moves,
}

pub struct Moves;

impl Mover for Moves {
    fn move_pairs(&self, running: &mut Running, pairs: &[(PathBuf, PathBuf)]) -> Moved {
        let (result, moved) = transfer_to(running, pairs, How::Move);
        (result, moved.into_iter().collect())
    }

    fn delete(&self, running: &mut Running, paths: &[PathBuf]) -> Moved {
        delete_paths(running, paths)
    }
}

impl nova_files_jobs::JobKind for Files {
    type Op = Operation;
    type Undo = Undo;

    fn claims(&self, operation: &Operation) -> bool {
        match operation {
            Operation::Copy { sources, .. } => copied_out(sources).is_some(),
            Operation::ChangeAccess { .. }
            | Operation::Trash { .. }
            | Operation::Restore { .. }
            | Operation::DeleteFromTrash { .. }
            | Operation::EmptyTrash
            | Operation::Undo { .. } => true,
            _ => false,
        }
    }

    fn run(&self, running: &mut Running, operation: &Operation, undo: Option<&Undo>) -> Ended {
        let trashes = &self.trashes;
        match operation {
            Operation::Copy { sources, into } => match copied_out(sources) {
                Some((archive, picked)) => extract(
                    running,
                    (self.archiver.as_ref(), &self.mover),
                    &archive,
                    into,
                    Taking::Picked(&picked),
                ),
                None => transfer(running, sources, into, How::Copy),
            },
            Operation::ChangeAccess { paths, change } => change_access(running, paths, change),
            Operation::Trash { paths } => trash(running, paths, trashes.home.as_deref()),
            Operation::Restore { items } => restore(running, items, trashes, &self.mover),
            Operation::DeleteFromTrash { items } => {
                delete_from_trash(running, items, trashes, &self.mover)
            }
            Operation::EmptyTrash => empty_trash(running, trashes, &self.mover),
            _ => undo::run(running, undo.cloned(), trashes, &self.mover),
        }
    }

    fn rest(&self, operation: &Operation, control: &Control) -> Result<Operation, Cancelled> {
        still_to_do(operation, control)
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
        first_path(operation)
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

fn still_to_do(operation: &Operation, control: &Control) -> Result<Operation, Cancelled> {
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

fn first_path(operation: &Operation) -> PathBuf {
    match operation {
        Operation::Copy { sources, into } => sources.first().unwrap_or(into).clone(),
        Operation::Trash { paths }
        | Operation::ChangeAccess { paths, .. }
        | Operation::Restore { items: paths }
        | Operation::DeleteFromTrash { items: paths } => paths.first().cloned().unwrap_or_default(),
        _ => PathBuf::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let kinds = kinds(Trashes::default(), None);
        let first_path = |operation| nova_files_jobs::JobKind::first_path(&kinds, &operation);

        assert_eq!(first_path(rename), Path::new("/a/b"));
        assert_eq!(first_path(folder), Path::new("/a/new"));
        assert_eq!(first_path(copy(vec!["/s".into()])), Path::new("/s"));
        assert_eq!(first_path(copy(Vec::new())), Path::new("/into"));
    }
}
