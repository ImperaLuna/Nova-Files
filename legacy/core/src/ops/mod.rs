mod dropped;
mod undo;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use nova_files_api::{Connector, EventSink, JobId, JobTag, Operation};
use nova_files_jobs::control::{Cancelled, Control};
use nova_files_jobs::kinds::Kinds;
use nova_files_jobs::mover::{Moved, Mover};
use nova_files_jobs::running::Running;
use nova_files_jobs::{Ended, lock};
use nova_files_jobs::{journal, plan};
use nova_files_jobs::{running, stop};
use nova_files_properties_core::change_access;

pub use dropped::answer as answer_drop_would;
#[cfg(test)]
pub use nova_files_jobs::test_folder::{self, TestFolder};

use crate::inside;
use crate::trash::Trashes;
use nova_files_archives_core::{Archiver, Asked, Taking, compress, extract};
use nova_files_delete_core::{delete, delete_paths};
use nova_files_rename_core::{new_folder, rename};
use nova_files_transfer_core::{How, transfer, transfer_to};
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
    let set_aside = Arc::new(Trasher::new(trashes.home.clone()));
    let files = Files {
        trashes,
        archiver,
        mover: Moves,
    };
    Jobs::new(
        state,
        Arc::new(Kinds::default().with(files)),
        set_aside,
        wake,
    )
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
                let operation = dropped::dropped(sources, into);
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

    fn run(&self, running: &mut Running, operation: &Operation, undo: Option<&Undo>) -> Ended {
        let trashes = &self.trashes;
        let archiver = self.archiver.as_ref();
        match operation {
            Operation::Rename { path, name } => rename(path, name),
            Operation::NewFolder { inside, name } => new_folder(inside, name),
            Operation::Copy { sources, into } => match copied_out(sources) {
                Some((archive, picked)) => extract(
                    running,
                    (archiver, &self.mover),
                    &archive,
                    into,
                    Taking::Picked(&picked),
                ),
                None => transfer(running, sources, into, How::Copy),
            },
            Operation::Move { sources, into } => transfer(running, sources, into, How::Move),
            Operation::Delete { paths } => delete(running, paths),
            Operation::ChangeAccess { paths, change } => change_access(running, paths, change),
            Operation::Trash { paths } => trash(running, paths, trashes.home.as_deref()),
            Operation::Restore { items } => restore(running, items, trashes, &self.mover),
            Operation::DeleteFromTrash { items } => {
                delete_from_trash(running, items, trashes, &self.mover)
            }
            Operation::EmptyTrash => empty_trash(running, trashes, &self.mover),
            Operation::Undo { .. } => undo::run(running, undo.cloned(), trashes, &self.mover),
            Operation::Extract { archive, into, to } => extract(
                running,
                (archiver, &self.mover),
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
                let asked = Asked {
                    sources,
                    into,
                    name,
                    packing: *packing,
                    level: *level,
                    locked: *locked,
                };
                compress(running, archiver, &asked)
            }
        }
    }

    fn rest(&self, operation: &Operation, control: &Control) -> Result<Operation, Cancelled> {
        still_to_do(operation, control)
    }

    fn undo(&self, running: &mut Running, operation: &Operation, ended: &Ended) -> Option<Undo> {
        let steps: Vec<_> = std::mem::take(&mut running.steps).into_iter().collect();
        undo_of(operation, &steps, &ended.0, &self.trashes)
            .map(|undo| undo::putting_back(undo, &running.replaced))
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
