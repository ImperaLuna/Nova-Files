use std::ffi::OsString;
use std::path::PathBuf;

use nova_files_api::{Failure, Operation, Outcome};
use nova_files_jobs::running::Running;
use nova_files_jobs::{Ended, JobKind};
use nova_files_rename_api::{NewFileJob, NewFolderJob, RenameJob};

use crate::undo::{self, Undo, undo_of};
use crate::{new_file, new_folder, rename};

pub struct Renames;

impl JobKind for Renames {
    type Op = Operation;
    type Undo = Undo;

    fn claims(&self, operation: &Operation) -> bool {
        operation.get::<RenameJob>().is_some()
            || operation.get::<NewFolderJob>().is_some()
            || operation.get::<NewFileJob>().is_some()
    }

    fn run(&self, running: &mut Running, operation: &Operation, undo: Option<&Undo>) -> Ended {
        if let Some(undo) = undo {
            return undo::run(running, undo);
        }
        if let Some(job) = operation.get::<RenameJob>() {
            return rename(&job.path, &job.name);
        }
        if let Some(job) = operation.get::<NewFolderJob>() {
            return new_folder(&job.inside, &job.name);
        }
        if let Some(job) = operation.get::<NewFileJob>() {
            return new_file(&job.inside, &job.name);
        }
        (
            Outcome::Failed {
                path: PathBuf::new(),
                reason: Failure::Other,
            },
            Vec::new(),
        )
    }

    fn undo(&self, _: &mut Running, operation: &Operation, ended: &Ended) -> Option<Undo> {
        undo_of(operation, &ended.0)
    }

    fn examples(&self) -> Vec<Operation> {
        vec![
            nova_files_rename_api::rename(PathBuf::new(), OsString::new()),
            nova_files_rename_api::new_folder(PathBuf::new(), OsString::new()),
            nova_files_rename_api::new_file(PathBuf::new(), OsString::new()),
        ]
    }

    fn first_path(&self, operation: &Operation) -> PathBuf {
        if let Some(job) = operation.get::<RenameJob>() {
            return job.path.clone();
        }
        operation
            .get::<NewFolderJob>()
            .map(|job| job.inside.join(&job.name))
            .or_else(|| {
                operation
                    .get::<NewFileJob>()
                    .map(|job| job.inside.join(&job.name))
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renames_claim_renaming_and_new_folders_and_name_where_they_start() {
        let rename = nova_files_rename_api::rename("/a/b".into(), "c".into());
        let folder = nova_files_rename_api::new_folder("/a".into(), "new".into());
        let file = nova_files_rename_api::new_file("/a".into(), "made".into());
        let delete = Operation::Undo {
            job: nova_files_api::JobId(1),
        };

        assert!(Renames.claims(&rename) && Renames.claims(&folder) && Renames.claims(&file));
        assert!(!Renames.claims(&delete));
        assert_eq!(Renames.first_path(&rename), PathBuf::from("/a/b"));
        assert_eq!(Renames.first_path(&folder), PathBuf::from("/a/new"));
        assert_eq!(Renames.first_path(&file), PathBuf::from("/a/made"));
        assert_eq!(Renames.first_path(&delete), PathBuf::new());
    }
}
