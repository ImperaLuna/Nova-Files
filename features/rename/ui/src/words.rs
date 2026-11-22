use std::path::{Path, PathBuf};

use nova_files_api::Tally;
use nova_files_rename_api::{NewFolderJob, RenameJob};
use nova_files_shell::island::job_words::{Doing, Failed, JobWords};

pub struct RenameWords;

impl JobWords<RenameJob> for RenameWords {
    fn doing(&self, _: &RenameJob) -> Doing {
        Doing {
            verb: "Renaming",
            after_items: "",
        }
    }

    fn sources<'a>(&self, job: &'a RenameJob) -> &'a [PathBuf] {
        std::slice::from_ref(&job.path)
    }

    fn done(&self, job: &RenameJob, _: Option<&Path>, _: Tally) -> Option<String> {
        Some(format!("Renamed to {}", job.name.to_string_lossy()))
    }

    fn failed(&self, _: &RenameJob) -> Failed {
        Failed {
            verb: "rename",
            names_item: true,
        }
    }
}

pub struct NewFolderWords;

impl JobWords<NewFolderJob> for NewFolderWords {
    fn doing(&self, _: &NewFolderJob) -> Doing {
        Doing {
            verb: "Making",
            after_items: "",
        }
    }

    fn sources<'a>(&self, job: &'a NewFolderJob) -> &'a [PathBuf] {
        std::slice::from_ref(&job.inside)
    }

    fn done(&self, _: &NewFolderJob, _: Option<&Path>, _: Tally) -> Option<String> {
        None
    }

    fn failed(&self, _: &NewFolderJob) -> Failed {
        Failed {
            verb: "make",
            names_item: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rename_names_the_new_name_and_a_new_folder_says_nothing() {
        let rename = RenameJob {
            path: PathBuf::from("/home/a/Music"),
            name: "Songs".into(),
        };
        let folder = NewFolderJob {
            inside: PathBuf::from("/home/a"),
            name: "New folder".into(),
        };
        let none = Tally::default();
        assert_eq!(
            RenameWords.done(&rename, None, none).as_deref(),
            Some("Renamed to Songs")
        );
        assert_eq!(NewFolderWords.done(&folder, None, none), None);
        assert_eq!(
            RenameWords.sources(&rename),
            [PathBuf::from("/home/a/Music")]
        );
        assert_eq!(NewFolderWords.sources(&folder), [PathBuf::from("/home/a")]);
        assert_eq!(NewFolderWords.failed(&folder).verb, "make");
    }
}
