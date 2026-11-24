use std::path::{Path, PathBuf};

use nova_files_api::{Failure, Operation, Outcome};
use nova_files_jobs::control::{Cancelled, Control};
use nova_files_jobs::rest::still_there;
use nova_files_jobs::running::Running;
use nova_files_jobs::undo::landed;
use nova_files_jobs::{Ended, JobKind};
use nova_files_transfer_api::{CopyJob, LinkJob, MoveJob, copy, journal, link, move_into};

use crate::links::make_links;
use crate::undo::{self, Undo, undo_of};
use crate::{How, transfer};

pub struct Transfers;

pub fn asked(operation: &Operation) -> Option<(&[PathBuf], &Path, How)> {
    operation
        .get::<CopyJob>()
        .map(|job| (job.sources.as_slice(), job.into.as_path(), How::Copy))
        .or_else(|| {
            operation
                .get::<MoveJob>()
                .map(|job| (job.sources.as_slice(), job.into.as_path(), How::Move))
        })
}

impl JobKind for Transfers {
    type Op = Operation;
    type Undo = Undo;

    fn claims(&self, operation: &Operation) -> bool {
        asked(operation).is_some() || operation.get::<LinkJob>().is_some()
    }

    fn run(&self, running: &mut Running, operation: &Operation, undo: Option<&Undo>) -> Ended {
        if let Some(undo) = undo {
            return undo::run(running, undo);
        }
        if let Some(job) = operation.get::<LinkJob>() {
            return make_links(running, &job.sources, &job.into);
        }
        match asked(operation) {
            Some((sources, into, how)) => transfer(running, sources, into, how),
            None => (
                Outcome::Failed {
                    path: PathBuf::new(),
                    reason: Failure::Other,
                },
                Vec::new(),
            ),
        }
    }

    fn rest(&self, operation: &Operation, control: &Control) -> Result<Operation, Cancelled> {
        Ok(match operation.get::<MoveJob>() {
            Some(job) => move_into(still_there(&job.sources, control)?, job.into.clone()),
            None => operation.clone(),
        })
    }

    fn undo(&self, running: &mut Running, operation: &Operation, _: &Ended) -> Option<Undo> {
        let how = asked(operation).map_or(How::Copy, |(_, _, how)| how);
        undo_of(how, landed(running))
    }

    fn examples(&self) -> Vec<Operation> {
        vec![
            copy(Vec::new(), PathBuf::new()),
            move_into(Vec::new(), PathBuf::new()),
            link(Vec::new(), PathBuf::new()),
        ]
    }

    fn first_path(&self, operation: &Operation) -> PathBuf {
        let (sources, into) = match (asked(operation), operation.get::<LinkJob>()) {
            (Some((sources, into, _)), _) => (sources, into),
            (None, Some(job)) => (job.sources.as_slice(), job.into.as_path()),
            (None, None) => return PathBuf::new(),
        };
        sources.first().map_or(into, PathBuf::as_path).to_path_buf()
    }

    fn resume(
        &self,
        word: &str,
        into: Option<PathBuf>,
        sources: Vec<PathBuf>,
    ) -> Option<Operation> {
        match (word, into) {
            (journal::COPY, Some(into)) => Some(copy(sources, into)),
            (journal::MOVE, Some(into)) => Some(move_into(sources, into)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfers_claim_copies_and_moves_and_start_at_the_first_source() {
        let copying = copy(vec!["/s".into()], "/into".into());
        let empty = move_into(Vec::new(), "/into".into());
        let delete = Operation::Undo {
            job: nova_files_api::JobId(1),
        };

        assert!(Transfers.claims(&copying) && Transfers.claims(&empty));
        assert!(!Transfers.claims(&delete));
        assert_eq!(Transfers.first_path(&copying), PathBuf::from("/s"));
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
        let moving = move_into(sources.clone(), into.clone());
        let copying = copy(sources, into.clone());

        assert_eq!(
            Transfers.rest(&moving, &Control::default()).unwrap(),
            move_into(vec![file], into)
        );
        assert_eq!(
            Transfers.rest(&copying, &Control::default()).unwrap(),
            copying
        );
    }

    #[test]
    fn a_copy_or_move_left_by_a_crash_is_known_again() {
        let into = PathBuf::from("/into");
        let sources = vec![PathBuf::from("/s")];
        assert_eq!(
            Transfers.resume(journal::COPY, Some(into.clone()), sources.clone()),
            Some(copy(sources.clone(), into.clone()))
        );
        assert_eq!(
            Transfers.resume(journal::MOVE, Some(into.clone()), sources.clone()),
            Some(move_into(sources.clone(), into))
        );
        assert_eq!(Transfers.resume(journal::COPY, None, sources), None);
        assert_eq!(Transfers.resume("trash", None, Vec::new()), None);
    }
}
