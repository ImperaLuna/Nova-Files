//! What a job's thread holds while it runs: its id, its switch, its
//! progress, its journal once it has one, and the answers a client gave
//! for all conflicts of a kind.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use nova_files_api::{Answer, Conflict, ConflictKind, JobId, Scope};

use super::control::Cancelled;
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
    /// Answers given for every conflict of their kind.
    answers: HashMap<ConflictKind, Answer>,
    /// How many conflicts it asked about.
    asked: u64,
    /// What the job made before a crash cut it short, taken as done now
    /// that it resumes.
    pub made_before: HashSet<PathBuf>,
    /// Whether the job resumes, adding to the journal it already has.
    resumes: bool,
}

impl Running {
    pub fn new(
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
            answers: HashMap::new(),
            asked: 0,
            made_before: HashSet::new(),
            resumes: false,
        }
    }

    /// The job resumes after a crash, having made `made` before it: its
    /// journal, which already says what the job does, is added to.
    pub fn resuming(&mut self, made: HashSet<PathBuf>) {
        self.made_before = made;
        self.resumes = true;
        if let Some(folder) = self.journals.take() {
            match Journal::reopen(&folder, self.job) {
                Ok(journal) => self.journal = Some(journal),
                Err(error) => log::warn!("job {} resumes without its journal: {error}", self.job.0),
            }
        }
    }

    /// Holds at `conflict` until a client answers, unless one already
    /// answered for all of its kind. Retry is never kept for a kind: what
    /// still fails would be retried for ever.
    ///
    /// # Errors
    /// [`Cancelled`] when a client cancels the job instead.
    pub fn ask(&mut self, mut conflict: Conflict) -> Result<Answer, Cancelled> {
        if let Some(&answer) = self.answers.get(&conflict.kind) {
            return Ok(answer);
        }
        self.asked += 1;
        conflict.number = self.asked;
        let kind = conflict.kind;
        self.reporter.conflict(conflict);
        let (answer, scope) = self.shared.control.answer_to()?;
        if scope == Scope::AllOfThisKind && answer != Answer::Retry {
            self.answers.insert(kind, answer);
        }
        Ok(answer)
    }

    /// Asks about `conflict`, of a kind that takes only Skip or Retry,
    /// and returns whether the answer is to skip it.
    ///
    /// # Errors
    /// [`Cancelled`] when a client cancels the job instead.
    pub fn skips(&mut self, conflict: Conflict) -> Result<bool, Cancelled> {
        Ok(self.ask(conflict)? == Answer::Skip)
    }

    /// Adds `record` to the job's journal, starting the journal with the
    /// first record. Without a journal the job still runs: its part files
    /// are hidden and a real name never holds half a file, so only a
    /// resume after a crash is lost. That is logged once.
    pub fn record(&mut self, record: Record<'_>) {
        let says_what = matches!(
            record,
            Record::Operation(_) | Record::Into(_) | Record::Source(_)
        );
        if self.resumes && says_what {
            return;
        }
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

#[cfg(test)]
impl Running {
    /// A job of its own, without a journal, for unit tests.
    pub fn for_test() -> Self {
        let shared = Arc::new(Job::new(nova_files_api::Operation::Copy {
            sources: Vec::new(),
            into: PathBuf::from("/"),
        }));
        let reporter = Reporter::new(
            super::audience::Audience::default(),
            Arc::default(),
            JobId(1),
            Arc::clone(&shared),
        );
        Self::new(JobId(1), shared, reporter, None)
    }
}
