use std::path::{Path, PathBuf};
use std::sync::Arc;

use nova_files_api::Connector;
use nova_files_jobs::kinds::Kinds;
use nova_files_jobs::mover::{Moved, Mover};
use nova_files_jobs::running::Running;
use nova_files_properties_core::AccessJobs;

use nova_files_archives_core::{Archiver, Archives};
use nova_files_delete_core::{Deletes, delete_paths};
use nova_files_rename_core::Renames;
use nova_files_transfer_core::{How, Transfers, transfer_to};
use nova_files_trash_core::{TrashJobs, Trasher, Trashes};

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
        let rename = nova_files_rename_api::rename("/a/b".into(), "c".into());
        let folder = nova_files_rename_api::new_folder("/a".into(), "new".into());
        let copy = |sources: Vec<PathBuf>| nova_files_transfer_api::copy(sources, "/into".into());

        let kinds = kinds(Trashes::default(), None);
        let first_path = |operation| nova_files_jobs::JobKind::first_path(&kinds, &operation);

        assert_eq!(first_path(rename), Path::new("/a/b"));
        assert_eq!(first_path(folder), Path::new("/a/new"));
        assert_eq!(first_path(copy(vec!["/s".into()])), Path::new("/s"));
        assert_eq!(first_path(copy(Vec::new())), Path::new("/into"));
    }
}
