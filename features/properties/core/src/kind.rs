use std::path::PathBuf;

use nova_files_api::Operation;
use nova_files_jobs::running::Running;
use nova_files_jobs::undo::gone;
use nova_files_jobs::{Ended, JobKind};

use crate::change_access;

pub struct AccessJobs;

impl JobKind for AccessJobs {
    type Op = Operation;
    type Undo = ();

    fn claims(&self, operation: &Operation) -> bool {
        matches!(operation, Operation::ChangeAccess { .. })
    }

    fn run(&self, running: &mut Running, operation: &Operation, _: Option<&()>) -> Ended {
        match operation {
            Operation::ChangeAccess { paths, change } => change_access(running, paths, change),
            _ => gone(),
        }
    }

    fn first_path(&self, operation: &Operation) -> PathBuf {
        match operation {
            Operation::ChangeAccess { paths, .. } => paths.first().cloned().unwrap_or_default(),
            _ => PathBuf::new(),
        }
    }
}
