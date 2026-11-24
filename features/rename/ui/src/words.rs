use std::path::{Path, PathBuf};

use nova_files_api::Tally;
use nova_files_rename_api::{NewFileJob, NewFolderJob, RenameJob, RenameManyJob};
use nova_files_shell::island::job_words::{Doing, Failed, JobWords};
use nova_files_shell::island::words::what;

const RENAMING: Doing = Doing {
    verb: "Renaming",
    after_items: "",
};
const RENAME_FAILED: Failed = Failed {
    verb: "rename",
    names_item: true,
};
const MAKING: Doing = Doing {
    verb: "Making",
    after_items: "",
};
const MAKE_FAILED: Failed = Failed {
    verb: "make",
    names_item: true,
};

pub struct RenameWords;

impl JobWords<RenameJob> for RenameWords {
    fn doing(&self, _: &RenameJob) -> Doing {
        RENAMING
    }

    fn sources<'a>(&self, job: &'a RenameJob) -> &'a [PathBuf] {
        std::slice::from_ref(&job.path)
    }

    fn done(&self, job: &RenameJob, _: Option<&Path>, _: Tally) -> Option<String> {
        Some(format!("Renamed to {}", job.name.to_string_lossy()))
    }

    fn failed(&self, _: &RenameJob) -> Failed {
        RENAME_FAILED
    }
}

pub struct RenameManyWords;

impl JobWords<RenameManyJob> for RenameManyWords {
    fn doing(&self, _: &RenameManyJob) -> Doing {
        RENAMING
    }

    fn sources<'a>(&self, job: &'a RenameManyJob) -> &'a [PathBuf] {
        &job.paths
    }

    fn done(&self, job: &RenameManyJob, _: Option<&Path>, _: Tally) -> Option<String> {
        Some(format!("Renamed {}", what(&job.paths)))
    }

    fn failed(&self, _: &RenameManyJob) -> Failed {
        RENAME_FAILED
    }
}

pub struct NewFolderWords;

impl JobWords<NewFolderJob> for NewFolderWords {
    fn doing(&self, _: &NewFolderJob) -> Doing {
        MAKING
    }

    fn sources<'a>(&self, job: &'a NewFolderJob) -> &'a [PathBuf] {
        std::slice::from_ref(&job.inside)
    }

    fn done(&self, _: &NewFolderJob, _: Option<&Path>, _: Tally) -> Option<String> {
        None
    }

    fn failed(&self, _: &NewFolderJob) -> Failed {
        MAKE_FAILED
    }
}

pub struct NewFileWords;

impl JobWords<NewFileJob> for NewFileWords {
    fn doing(&self, _: &NewFileJob) -> Doing {
        MAKING
    }

    fn sources<'a>(&self, job: &'a NewFileJob) -> &'a [PathBuf] {
        std::slice::from_ref(&job.inside)
    }

    fn done(&self, _: &NewFileJob, _: Option<&Path>, _: Tally) -> Option<String> {
        None
    }

    fn failed(&self, _: &NewFileJob) -> Failed {
        MAKE_FAILED
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renaming_several_counts_them() {
        let many = RenameManyJob {
            paths: vec![PathBuf::from("/a/x"), PathBuf::from("/a/y")],
            names: vec!["1".into(), "2".into()],
        };
        assert_eq!(
            RenameManyWords
                .done(&many, None, Tally::default())
                .as_deref(),
            Some("Renamed 2 items")
        );
        assert_eq!(RenameManyWords.sources(&many).len(), 2);
        assert_eq!(RenameManyWords.failed(&many).verb, "rename");
    }

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
