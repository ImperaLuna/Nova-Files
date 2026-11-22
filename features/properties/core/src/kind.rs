use std::path::PathBuf;

use nova_files_api::Operation;
use nova_files_jobs::running::Running;
use nova_files_jobs::undo::gone;
use nova_files_jobs::{Ended, JobKind};
use nova_files_properties_api::ChangeAccessJob;

use crate::change_access;

pub struct AccessJobs;

impl JobKind for AccessJobs {
    type Op = Operation;
    type Undo = ();

    fn claims(&self, operation: &Operation) -> bool {
        operation.get::<ChangeAccessJob>().is_some()
    }

    fn run(&self, running: &mut Running, operation: &Operation, _: Option<&()>) -> Ended {
        operation
            .get::<ChangeAccessJob>()
            .map_or_else(gone, |job| change_access(running, &job.paths, &job.change))
    }

    fn examples(&self) -> Vec<Operation> {
        vec![nova_files_properties_api::change_access(
            Vec::new(),
            nova_files_properties_api::AccessChange::default(),
        )]
    }

    fn first_path(&self, operation: &Operation) -> PathBuf {
        operation
            .get::<ChangeAccessJob>()
            .and_then(|job| job.paths.first().cloned())
            .unwrap_or_default()
    }
}
