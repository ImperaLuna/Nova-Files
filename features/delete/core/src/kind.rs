use std::path::PathBuf;

use nova_files_api::{Failure, Operation, Outcome};
use nova_files_delete_api::{DeleteJob, delete as deleting, journal};
use nova_files_jobs::control::{Cancelled, Control};
use nova_files_jobs::rest::still_there;
use nova_files_jobs::running::Running;
use nova_files_jobs::{Ended, JobKind};

use crate::delete;

pub struct Deletes;

impl JobKind for Deletes {
    type Op = Operation;
    type Undo = ();

    fn claims(&self, operation: &Operation) -> bool {
        operation.get::<DeleteJob>().is_some()
    }

    fn run(&self, running: &mut Running, operation: &Operation, _: Option<&()>) -> Ended {
        operation.get::<DeleteJob>().map_or_else(
            || {
                (
                    Outcome::Failed {
                        path: PathBuf::new(),
                        reason: Failure::Other,
                    },
                    Vec::new(),
                )
            },
            |job| delete(running, &job.paths),
        )
    }

    fn rest(&self, operation: &Operation, control: &Control) -> Result<Operation, Cancelled> {
        Ok(match operation.get::<DeleteJob>() {
            Some(job) => deleting(still_there(&job.paths, control)?),
            None => operation.clone(),
        })
    }

    fn first_path(&self, operation: &Operation) -> PathBuf {
        operation
            .get::<DeleteJob>()
            .and_then(|job| job.paths.first().cloned())
            .unwrap_or_default()
    }

    fn resume(&self, name: &str, into: Option<PathBuf>, paths: Vec<PathBuf>) -> Option<Operation> {
        (name == journal::DELETE && into.is_none()).then(|| deleting(paths))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletes_claim_only_deleting_and_start_at_the_first_path() {
        let delete = deleting(vec!["/a".into(), "/b".into()]);
        let nothing = deleting(Vec::new());
        let other = Operation::Undo {
            job: nova_files_api::JobId(1),
        };

        assert!(Deletes.claims(&delete));
        assert!(!Deletes.claims(&other));
        assert_eq!(Deletes.first_path(&delete), PathBuf::from("/a"));
        assert_eq!(Deletes.first_path(&nothing), PathBuf::new());
        assert_eq!(Deletes.first_path(&other), PathBuf::new());
    }

    #[test]
    fn a_resumed_delete_leaves_out_what_is_gone() {
        let folder = nova_files_test_folder::TestFolder::new("delete-kind-rest");
        let file = folder.file("file", "f");
        let gone = folder.path().join("gone");
        let delete = deleting(vec![file.clone(), gone]);

        let left = Deletes.rest(&delete, &Control::default()).unwrap();

        assert_eq!(left, deleting(vec![file]));
    }

    #[test]
    fn a_delete_left_by_a_crash_is_known_again() {
        let paths = vec![PathBuf::from("/a")];
        assert_eq!(
            Deletes.resume(journal::DELETE, None, paths.clone()),
            Some(deleting(paths.clone()))
        );
        assert_eq!(
            Deletes.resume(journal::DELETE, Some("/b".into()), paths),
            None
        );
    }
}
