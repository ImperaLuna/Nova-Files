use std::path::{Path, PathBuf};

use nova_files_api::Tally;
use nova_files_delete_api::DeleteJob;
use nova_files_shell::island::job_words::{Doing, Failed, JobWords};
use nova_files_shell::island::words::what;

pub struct DeleteWords;

impl JobWords<DeleteJob> for DeleteWords {
    fn doing(&self, _: &DeleteJob) -> Doing {
        Doing {
            verb: "Deleting",
            after_items: "",
        }
    }

    fn sources<'a>(&self, job: &'a DeleteJob) -> &'a [PathBuf] {
        &job.paths
    }

    fn done(&self, job: &DeleteJob, _: Option<&Path>, _: Tally) -> Option<String> {
        Some(format!("Deleted {}", what(&job.paths)))
    }

    fn failed(&self, _: &DeleteJob) -> Failed {
        Failed {
            verb: "delete",
            names_item: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_delete_names_what_went() {
        let one = DeleteJob {
            paths: vec![PathBuf::from("/home/a/notes.txt")],
        };
        let two = DeleteJob {
            paths: vec![PathBuf::from("/a"), PathBuf::from("/b")],
        };
        let none = Tally::default();
        assert_eq!(
            DeleteWords.done(&one, None, none).as_deref(),
            Some("Deleted notes.txt")
        );
        assert_eq!(
            DeleteWords.done(&two, None, none).as_deref(),
            Some("Deleted 2 items")
        );
        assert_eq!(DeleteWords.sources(&two).len(), 2);
    }
}
