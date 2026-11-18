use std::path::PathBuf;

use nova_files_api::{Failure, JobId, Operation, Outcome};
use nova_files_jobs::control::{Cancelled, Control};
use nova_files_jobs::running::Running;
use nova_files_jobs::{Ended, JobKind};

use crate::{How, transfer};

pub struct Transfers;

impl JobKind for Transfers {
    type Op = Operation;
    type Undo = ();

    fn claims(&self, operation: &Operation) -> bool {
        matches!(operation, Operation::Copy { .. } | Operation::Move { .. })
    }

    fn run(&self, running: &mut Running, operation: &Operation, _: Option<&()>) -> Ended {
        match operation {
            Operation::Copy { sources, into } => transfer(running, sources, into, How::Copy),
            Operation::Move { sources, into } => transfer(running, sources, into, How::Move),
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
            Operation::Copy { sources, into } | Operation::Move { sources, into } => {
                sources.first().unwrap_or(into).clone()
            }
            _ => PathBuf::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfers_claim_copies_and_moves_and_start_at_the_first_source() {
        let copy = Operation::Copy {
            sources: vec!["/s".into()],
            into: "/into".into(),
        };
        let empty = Operation::Move {
            sources: Vec::new(),
            into: "/into".into(),
        };
        let delete = Operation::Delete {
            paths: vec!["/a".into()],
        };

        assert!(Transfers.claims(&copy) && Transfers.claims(&empty));
        assert!(!Transfers.claims(&delete));
        assert_eq!(Transfers.first_path(&copy), PathBuf::from("/s"));
        assert_eq!(Transfers.first_path(&empty), PathBuf::from("/into"));
        assert_eq!(Transfers.first_path(&delete), PathBuf::new());
    }
}
