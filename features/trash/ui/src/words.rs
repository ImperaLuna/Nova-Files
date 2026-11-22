use std::path::{Path, PathBuf};

use nova_files_api::Tally;
use nova_files_shell::island::job_words::{Doing, Failed, JobWords};
use nova_files_shell::island::words::{ITEM, ITEMS, count, what};
use nova_files_trash_api::{DeleteFromTrashJob, EmptyTrashJob, RestoreJob, TrashJob};

use crate::TRASH_LABEL;

const fn doing(verb: &'static str, after_items: &'static str) -> Doing {
    Doing { verb, after_items }
}

const fn failed(verb: &'static str, names_item: bool) -> Failed {
    Failed { verb, names_item }
}

pub struct TrashWords;

impl JobWords<TrashJob> for TrashWords {
    fn doing(&self, _: &TrashJob) -> Doing {
        doing("Moving", " to Trash")
    }

    fn sources<'a>(&self, job: &'a TrashJob) -> &'a [PathBuf] {
        &job.paths
    }

    fn done(&self, job: &TrashJob, _: Option<&Path>, _: Tally) -> Option<String> {
        Some(format!("Moved {} to {TRASH_LABEL}", what(&job.paths)))
    }

    fn failed(&self, _: &TrashJob) -> Failed {
        failed("move to Trash", true)
    }
}

pub struct RestoreWords;

impl JobWords<RestoreJob> for RestoreWords {
    fn doing(&self, _: &RestoreJob) -> Doing {
        doing("Restoring", "")
    }

    fn sources<'a>(&self, job: &'a RestoreJob) -> &'a [PathBuf] {
        &job.items
    }

    fn done(&self, job: &RestoreJob, _: Option<&Path>, _: Tally) -> Option<String> {
        Some(format!("Restored {}", count(job.items.len(), ITEM, ITEMS)))
    }

    fn failed(&self, _: &RestoreJob) -> Failed {
        failed("restore", true)
    }
}

pub struct DeleteFromTrashWords;

impl JobWords<DeleteFromTrashJob> for DeleteFromTrashWords {
    fn doing(&self, _: &DeleteFromTrashJob) -> Doing {
        doing("Deleting", "")
    }

    fn sources<'a>(&self, job: &'a DeleteFromTrashJob) -> &'a [PathBuf] {
        &job.items
    }

    fn done(&self, job: &DeleteFromTrashJob, _: Option<&Path>, _: Tally) -> Option<String> {
        Some(format!("Deleted {}", count(job.items.len(), ITEM, ITEMS)))
    }

    fn failed(&self, _: &DeleteFromTrashJob) -> Failed {
        failed("delete", true)
    }
}

pub struct EmptyTrashWords;

impl JobWords<EmptyTrashJob> for EmptyTrashWords {
    fn doing(&self, _: &EmptyTrashJob) -> Doing {
        doing("Deleting", "")
    }

    fn sources<'a>(&self, _: &'a EmptyTrashJob) -> &'a [PathBuf] {
        &[]
    }

    fn done(&self, _: &EmptyTrashJob, _: Option<&Path>, _: Tally) -> Option<String> {
        Some(format!("{TRASH_LABEL} emptied"))
    }

    fn failed(&self, _: &EmptyTrashJob) -> Failed {
        failed("empty Trash", false)
    }
}

#[cfg(test)]
#[path = "words_tests.rs"]
mod tests;
