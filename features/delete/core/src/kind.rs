use std::path::PathBuf;

use nova_files_api::{Failure, JobId, Operation, Outcome};
use nova_files_jobs::control::{Cancelled, Control};
use nova_files_jobs::running::Running;
use nova_files_jobs::{Ended, JobKind};

use crate::delete;

pub struct Deletes;

impl JobKind for Deletes {
    type Op = Operation;
    type Undo = ();

    fn claims(&self, operation: &Operation) -> bool {
        matches!(operation, Operation::Delete { .. })
    }

    fn run(&self, running: &mut Running, operation: &Operation, _: Option<&()>) -> Ended {
        match operation {
            Operation::Delete { paths } => delete(running, paths),
            _ => (
                Outcome::Failed {
                    path: PathBuf::new(),
                    reason: Failure::Other,
                },
                Vec::new(),
            ),
        }
    }

    fn rest(&self, operation: &Operation, _: &Control) -> Result<Operation, Cancelled> {
        Ok(operation.clone())
    }

    fn undo(&self, _: &mut Running, _: &Operation, _: &Ended) -> Option<()> {
        None
    }

    fn undoes(&self, _: &Operation) -> Option<JobId> {
        None
    }

    fn first_path(&self, operation: &Operation) -> PathBuf {
        match operation {
            Operation::Delete { paths } => paths.first().cloned().unwrap_or_default(),
            _ => PathBuf::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletes_claim_only_deleting_and_start_at_the_first_path() {
        let delete = Operation::Delete {
            paths: vec!["/a".into(), "/b".into()],
        };
        let nothing = Operation::Delete { paths: Vec::new() };
        let trash = Operation::Trash {
            paths: vec!["/a".into()],
        };

        assert!(Deletes.claims(&delete));
        assert!(!Deletes.claims(&trash));
        assert_eq!(Deletes.first_path(&delete), PathBuf::from("/a"));
        assert_eq!(Deletes.first_path(&nothing), PathBuf::new());
        assert_eq!(Deletes.first_path(&trash), PathBuf::new());
    }
}
