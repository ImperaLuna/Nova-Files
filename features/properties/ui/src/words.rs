use std::path::{Path, PathBuf};

use nova_files_api::Tally;
use nova_files_properties_api::ChangeAccessJob;
use nova_files_shell::island::job_words::{Doing, Failed, JobWords};

pub struct ChangeAccessWords;

impl JobWords<ChangeAccessJob> for ChangeAccessWords {
    fn doing(&self, _: &ChangeAccessJob) -> Doing {
        Doing {
            verb: "Changing",
            after_items: "",
        }
    }

    fn sources<'a>(&self, job: &'a ChangeAccessJob) -> &'a [PathBuf] {
        &job.paths
    }

    fn done(&self, _: &ChangeAccessJob, _: Option<&Path>, _: Tally) -> Option<String> {
        None
    }

    fn failed(&self, _: &ChangeAccessJob) -> Failed {
        Failed {
            verb: "change",
            names_item: true,
        }
    }
}
