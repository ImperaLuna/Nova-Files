//! What a job's thread holds while it runs: its id, its switch, its
//! progress, its journal once it has one, and the answers a client gave
//! for all conflicts of a kind.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nova_files_api::{Answer, Conflict, ConflictKind, Failure, JobId, Scope};

use super::clash::conflict_for;
use super::control::Cancelled;
use super::job::Job;
use super::journal::{Journal, Record};
use super::left::{Done, Left};
use super::plan::{Kind, kind};
use super::progress::Reporter;
use super::stop::Stop;

pub struct Running {
    pub job: JobId,
    /// The id in the names of the job's part files and backups: the job's
    /// own, or, once resumed, a new one, so none is a name a crash left.
    pub names: u64,
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
    /// What the job did before a crash cut it short, taken as done now
    /// that it resumes.
    pub before: Done,
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
            names: job.0,
            shared,
            reporter,
            journals,
            journal: None,
            answers: HashMap::new(),
            asked: 0,
            before: Done::default(),
            resumes: false,
        }
    }

    /// The job resumes after a crash, which `left` it as it was: its
    /// journal, which already says what the job does, is added to, and
    /// its hidden names take the id `names`.
    pub fn resuming(&mut self, left: Left, names: u64) {
        self.before = left.done;
        self.names = names;
        self.resumes = true;
        self.journals = None;
        match Journal::reopen(left.file, left.journal, left.whole) {
            Ok(journal) => self.journal = Some(journal),
            Err(error) => log::warn!("job {} resumes without its journal: {error}", self.job.0),
        }
    }

    /// Where the item from `from`, a `planned` kind, went before a crash,
    /// if it is still there as that kind. A folder swapped for a link
    /// since is not the job's, so it goes through the conflicts again.
    pub fn made_before(&self, from: &Path, planned: Kind) -> Option<&Path> {
        self.before
            .made
            .get(from)
            .map(PathBuf::as_path)
            .filter(|made| {
                made.symlink_metadata().is_ok_and(|there| {
                    std::mem::discriminant(&kind(&there)) == std::mem::discriminant(&planned)
                })
            })
    }

    /// Makes sure what the journal says so far is on the disk.
    pub fn settle(&mut self) {
        if let Some(journal) = &mut self.journal
            && let Err(error) = journal.sync()
        {
            self.drop_journal(&error);
        }
    }

    /// The journal failed with `error`. A journal that misses a step
    /// would mislead a resume, so it goes.
    fn drop_journal(&mut self, error: &io::Error) {
        log::warn!("job {} drops its journal: {error}", self.job.0);
        if let Some(journal) = self.journal.take() {
            journal.finish();
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

    /// Takes `step` on `path` until it works, asking the user what to do
    /// each time it fails for a reason theirs to answer. `None` when they
    /// skip it.
    ///
    /// # Errors
    /// [`Stop`] when a client cancels the job, or when the failure isn't
    /// the user's to answer.
    pub fn retry_or_skip<T>(
        &mut self,
        path: &Path,
        mut step: impl FnMut() -> Result<T, Failure>,
    ) -> Result<Option<T>, Stop> {
        loop {
            self.shared.control.checkpoint()?;
            let reason = match step() {
                Ok(done) => return Ok(Some(done)),
                Err(reason) => reason,
            };
            let Some(kind) = conflict_for(reason, path, path) else {
                return Err(Stop::because(path, reason));
            };
            let conflict = Conflict {
                kind,
                source: path.to_path_buf(),
                target: path.to_path_buf(),
                // Numbered when asked.
                number: 0,
            };
            if self.skips(conflict)? {
                return Ok(None);
            }
        }
    }

    /// Adds `record` to the job's journal, starting the journal with the
    /// first record. Without a journal the job still runs: its part files
    /// are hidden and a real name never holds half a file, so only a
    /// resume after a crash is lost. That is logged once.
    pub fn record(&mut self, record: Record<'_>) {
        let says_what = matches!(
            record,
            Record::Operation(_) | Record::Into(_) | Record::Source(_) | Record::SourcesDone
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
            self.drop_journal(&error);
        }
    }

    /// Records what the job is asked to do: operation `name`, the folder
    /// `into` when it has one, and each of `sources`.
    pub fn record_job(&mut self, name: &'static str, into: Option<&Path>, sources: &[PathBuf]) {
        self.record(Record::Operation(name));
        if let Some(into) = into {
            self.record(Record::Into(into));
        }
        for source in sources {
            self.record(Record::Source(source));
        }
        self.record(Record::SourcesDone);
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

#[cfg(test)]
mod tests;
