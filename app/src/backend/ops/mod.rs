use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use nova_files_api::{Connector, EventSink, JobTag, Operation};
use nova_files_jobs::kinds::Kinds;
use nova_files_jobs::lock;
use nova_files_jobs::mover::{Moved, Mover};
use nova_files_jobs::running::Running;
use nova_files_properties_core::AccessJobs;

use nova_files_archives_core::{Archiver, Archives};
use nova_files_delete_core::{Deletes, delete_paths};
use nova_files_rename_core::Renames;
use nova_files_transfer_core::{How, Transfers, transfer_to};
use nova_files_trash_core::{TrashJobs, Trasher, Trashes};

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
    Kinds::default()
        .with(TrashJobs {
            trashes,
            files: Moves,
        })
        .with(AccessJobs)
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
