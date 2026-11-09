//! What a job's thread holds while it runs: its id, its switch, its
//! progress, and its journal once it has one.

use std::path::PathBuf;
use std::sync::Arc;

use nova_files_api::JobId;

use super::job::Job;
use super::journal::{Journal, Record};
use super::progress::Reporter;

pub struct Running {
    pub job: JobId,
    /// What the core keeps of the job while it runs.
    pub shared: Arc<Job>,
    pub reporter: Reporter,
    /// Where journals go; `None` without a state folder.
    journals: Option<PathBuf>,
    journal: Option<Journal>,
}

impl Running {
    pub const fn new(
        job: JobId,
        shared: Arc<Job>,
        reporter: Reporter,
        journals: Option<PathBuf>,
    ) -> Self {
        Self {
            job,
            shared,
            reporter,
            journals,
            journal: None,
        }
    }

    /// Adds `record` to the job's journal, starting the journal with the
    /// first record. Without a journal the job still runs: its part files
    /// are hidden and a real name never holds half a file, so only a
    /// resume after a crash is lost. That is logged once.
    pub fn record(&mut self, record: Record<'_>) {
        if self.journal.is_none() {
            let Some(folder) = self.journals.take() else {
                return;
            };
            match Journal::start(&folder, self.job) {
                Ok(journal) => self.journal = Some(journal),
                Err(error) => {
                    log::warn!("job {} runs without a journal: {error}", self.job.0);
                    return;
                }
            }
        }
        if let Some(journal) = &mut self.journal
            && let Err(error) = journal.record(record)
        {
            // A journal that misses a step would mislead a resume.
            log::warn!("job {} drops its journal: {error}", self.job.0);
            if let Some(journal) = self.journal.take() {
                journal.finish();
            }
        }
    }

    /// The job is over: its journal goes.
    pub fn finish(mut self) {
        if let Some(journal) = self.journal.take() {
            journal.finish();
        }
    }
}
