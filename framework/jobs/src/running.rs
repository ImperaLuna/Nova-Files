use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io;
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nova_files_api::{Answer, Conflict, ConflictKind, Failure, JobId, Outcome, Scope, Tally};

use super::clash::conflict_for;
use super::control::Cancelled;
use super::folders::Folders;
use super::job::Job;
use super::journal::{Journal, Record, remove_journal, word};
use super::left::{Done, Left};
use super::plan::{Kind, kind};
use super::progress::Reporter;
use super::runner::Ended;
use super::set_aside::{Aside, SetAsideForJob};
use super::stop::Stop;

pub struct Running {
    pub job: JobId,
    pub names: u64,
    pub shared: Arc<Job>,
    pub reporter: Reporter,
    journals: Option<PathBuf>,
    journal: Option<Journal>,
    held: Option<(PathBuf, File)>,
    answers: HashMap<ConflictKind, Answer>,
    asked: u64,
    pub folders: Folders,
    pub before: Done,
    resumes: bool,
    sources: HashSet<PathBuf>,
    pub steps: BTreeMap<PathBuf, PathBuf>,
    pub tally: Tally,
    pub replaced: Vec<(PathBuf, PathBuf)>,
    aside: Option<Box<dyn SetAsideForJob>>,
    placed: HashSet<PathBuf>,
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
            held: None,
            answers: HashMap::new(),
            asked: 0,
            folders: Folders::default(),
            before: Done::default(),
            resumes: false,
            sources: HashSet::new(),
            steps: BTreeMap::new(),
            tally: Tally::default(),
            replaced: Vec::new(),
            aside: None,
            placed: HashSet::new(),
        }
    }

    pub fn set_aside_with(&mut self, aside: Box<dyn SetAsideForJob>) {
        self.aside = Some(aside);
    }

    pub fn set_aside(&mut self, at: (usize, &Path), shown: &Path) -> Aside {
        self.with_aside(|aside, running| aside.set_aside(running, at, shown))
            .unwrap_or(Aside::Nowhere)
    }

    pub fn trash(&mut self, paths: &[PathBuf]) -> Ended {
        self.with_aside(|aside, running| aside.trash(running, paths))
            .unwrap_or_else(|| {
                let path = paths.first().cloned().unwrap_or_default();
                let outcome = Outcome::Failed {
                    path,
                    reason: Failure::Unsupported,
                };
                (outcome, Vec::new())
            })
    }

    pub fn put_back(&mut self, items: &[PathBuf]) -> Option<Ended> {
        self.with_aside(|aside, running| aside.put_back(running, items))
            .flatten()
    }

    fn with_aside<T>(
        &mut self,
        act: impl FnOnce(&mut dyn SetAsideForJob, &mut Self) -> T,
    ) -> Option<T> {
        let mut aside = self.aside.take()?;
        let done = act(aside.as_mut(), self);
        self.aside = Some(aside);
        Some(done)
    }

    pub fn resuming(&mut self, left: Left, names: u64) {
        self.before = left.done;
        self.names = names;
        self.resumes = true;
        self.journals = None;
        let held = left.file.try_clone();
        let path = left.journal.clone();
        match Journal::reopen(left.file, left.journal, left.whole) {
            Ok(journal) => self.journal = Some(journal),
            Err(error) => {
                log::warn!("job {} resumes without its journal: {error}", self.job.0);
                match held {
                    Ok(file) => self.held = Some((path, file)),
                    Err(error) => log::warn!("{} is let go: {error}", path.display()),
                }
            }
        }
    }

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

    pub fn settle(&mut self) {
        if let Some(journal) = &mut self.journal
            && let Err(error) = journal.sync()
        {
            self.drop_journal(&error);
        }
    }

    fn drop_journal(&mut self, error: &io::Error) {
        log::warn!("job {} stops its journal: {error}", self.job.0);
        if let Some(journal) = self.journal.take() {
            self.held = Some(journal.into_held());
        }
    }

    pub fn answered(&self, kind: ConflictKind) -> bool {
        self.answers.contains_key(&kind)
    }

    pub fn ask(&mut self, mut conflict: Conflict) -> Result<Answer, Cancelled> {
        if let Some(&answer) = self.answers.get(&conflict.kind) {
            return Ok(answer);
        }
        self.settle();
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

    pub fn skips(&mut self, conflict: Conflict) -> Result<bool, Cancelled> {
        Ok(self.ask(conflict)? == Answer::Skip)
    }

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
                number: 0,
                more: 0,
            };
            if self.skips(conflict)? {
                return Ok(None);
            }
        }
    }

    pub fn record(&mut self, record: Record<'_>) {
        if let Record::Made(to, from) | Record::Moved(from, to) | Record::Trashed(from, to) = record
        {
            self.step(from, to);
        }
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

    pub fn record_backup(&mut self, backup: &Path, of: &Path) -> Result<(), Stop> {
        self.record(Record::Backup(backup, of));
        if self.journal.is_some() {
            Ok(())
        } else {
            Err(Stop::because(of, Failure::Other))
        }
    }

    pub fn record_job(&mut self, name: &'static str, into: Option<&Path>, sources: &[PathBuf]) {
        self.record(Record::Operation(name));
        if let Some(into) = into {
            self.record(Record::Into(into));
        }
        for source in sources {
            self.record(Record::Source(source));
        }
        if matches!(name, word::COPY | word::MOVE | word::TRASH | word::RESTORE) {
            self.sources = sources.iter().cloned().collect();
        }
        self.record(Record::SourcesDone);
    }

    fn step(&mut self, from: &Path, to: &Path) {
        if !from.ancestors().any(|path| self.sources.contains(path))
            || from
                .ancestors()
                .skip(1)
                .any(|path| self.steps.contains_key(path))
        {
            return;
        }
        let inside: Vec<_> = self
            .steps
            .range::<Path, _>((Bound::Excluded(from), Bound::Unbounded))
            .map(|(kept, _)| kept)
            .take_while(|kept| kept.starts_with(from))
            .cloned()
            .collect();
        for kept in inside {
            self.steps.remove(&kept);
        }
        self.steps.insert(from.to_path_buf(), to.to_path_buf());
        self.placed.insert(to.to_path_buf());
    }

    pub fn placed_here(&self, path: &Path) -> bool {
        path.ancestors().any(|path| self.placed.contains(path))
    }

    pub const fn resumed(&self) -> bool {
        self.resumes
    }

    pub fn finish(mut self) {
        if let Some(journal) = self.journal.take() {
            journal.finish();
        }
        if let Some((path, _held)) = self.held.take() {
            remove_journal(&path);
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Running {
    pub fn for_test() -> Self {
        let shared = Arc::new(Job::new(nova_files_api::Operation::Copy {
            sources: Vec::new(),
            into: PathBuf::from("/"),
        }));
        let reporter = Reporter::new(
            crate::audience::Audience::default(),
            Arc::default(),
            JobId(1),
            Arc::clone(&shared),
        );
        Self::new(JobId(1), shared, reporter, None)
    }

    pub fn with_journal(journal: Journal) -> Self {
        let mut running = Self::for_test();
        running.journal = Some(journal);
        running
    }

    pub fn journal_synced(&self) -> Option<std::time::Instant> {
        self.journal.as_ref().map(Journal::synced)
    }
}

#[cfg(test)]
mod tests;
