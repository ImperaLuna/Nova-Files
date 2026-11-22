use std::path::PathBuf;

use nova_files_api::{Failure, Operation, Outcome};
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

    fn rest(&self, operation: &Operation, control: &Control) -> Result<Operation, Cancelled> {
        Ok(match operation {
            Operation::Delete { paths } => Operation::Delete {
                paths: still_there(paths, control)?,
            },
            other => other.clone(),
        })
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

    #[test]
    fn a_resumed_delete_leaves_out_what_is_gone() {
        let folder = nova_files_test_folder::TestFolder::new("delete-kind-rest");
        let file = folder.file("file", "f");
        let gone = folder.path().join("gone");
        let delete = Operation::Delete {
            paths: vec![file.clone(), gone],
        };

        let left = Deletes.rest(&delete, &Control::default()).unwrap();

        assert_eq!(left, Operation::Delete { paths: vec![file] });
    }
}
