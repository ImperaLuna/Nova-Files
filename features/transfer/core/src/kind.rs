use std::path::PathBuf;

use nova_files_api::{Failure, Operation, Outcome};
use nova_files_jobs::control::{Cancelled, Control};
use nova_files_jobs::rest::still_there;
use nova_files_jobs::running::Running;
use nova_files_jobs::undo::landed;
use nova_files_jobs::{Ended, JobKind};

use crate::undo::{self, Undo, undo_of};
use crate::{How, transfer};

pub struct Transfers;

impl JobKind for Transfers {
    type Op = Operation;
    type Undo = Undo;

    fn claims(&self, operation: &Operation) -> bool {
        matches!(operation, Operation::Copy { .. } | Operation::Move { .. })
    }

    fn run(&self, running: &mut Running, operation: &Operation, undo: Option<&Undo>) -> Ended {
        if let Some(undo) = undo {
            return undo::run(running, undo);
        }
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

    fn rest(&self, operation: &Operation, control: &Control) -> Result<Operation, Cancelled> {
        Ok(match operation {
            Operation::Move { sources, into } => Operation::Move {
                sources: still_there(sources, control)?,
                into: into.clone(),
            },
            other => other.clone(),
        })
    }

    fn undo(&self, running: &mut Running, operation: &Operation, _: &Ended) -> Option<Undo> {
        undo_of(operation, landed(running))
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

    #[test]
    fn a_resumed_move_leaves_out_what_is_gone_and_a_copy_keeps_all() {
        let folder = nova_files_test_folder::TestFolder::new("transfer-kind-rest");
        let file = folder.file("file", "f");
        let gone = folder.path().join("gone");
        let into = folder.path().join("into");
        let sources = vec![file.clone(), gone];
        let moving = Operation::Move {
            sources: sources.clone(),
            into: into.clone(),
        };
        let copying = Operation::Copy {
            sources,
            into: into.clone(),
        };

        assert_eq!(
            Transfers.rest(&moving, &Control::default()).unwrap(),
            Operation::Move {
                sources: vec![file],
                into
            }
        );
        assert_eq!(
            Transfers.rest(&copying, &Control::default()).unwrap(),
            copying
        );
    }
}
