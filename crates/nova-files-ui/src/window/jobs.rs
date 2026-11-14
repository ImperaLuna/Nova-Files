//! The jobs this window starts, and what the core says about every job.
//!
//! A job starts with a tag the window picks; the core's `JobStarted`
//! names the job that tag became, and from then on the window follows it
//! by id. When any job ends, a folder it changed that is on screen is read
//! again, so what changed shows. When one of this window's own jobs ends,
//! the island says how it went, with Undo when the core said the job can
//! be undone (see `island`). Jobs other windows started are only read
//! for what they changed.

use std::collections::HashMap;
use std::path::PathBuf;

use gpui::Context;
use nova_files_api::{Command, Event, JobId, JobTag, Operation, Outcome};

use super::MainWindow;
use crate::island::{self, NoteUndo};

/// What the window knows about the jobs it started.
#[derive(Debug, Default)]
pub struct Jobs {
    next_tag: u64,
    /// Jobs asked for, by tag, until the core says which job each is.
    asked: HashMap<JobTag, Operation>,
    /// This window's jobs while they run.
    running: HashMap<JobId, Operation>,
    /// This window's jobs that can still be undone, oldest first.
    undoable: Vec<JobId>,
    /// This window's finished jobs the core stopped offering to undo, the
    /// newest last: an undo that fails or is cancelled offers its job
    /// again. At most [`UNDO_KEPT`].
    undo_gone: Vec<JobId>,
}

/// How many jobs the core keeps to undo; the window keeps as many it
/// may offer again.
const UNDO_KEPT: usize = 50;

impl Jobs {
    /// The newest of this window's jobs that can still be undone.
    pub fn last_undoable(&self) -> Option<JobId> {
        self.undoable.last().copied()
    }

    /// A tag for the next job asked for, new each time.
    const fn next_tag(&mut self) -> JobTag {
        let tag = JobTag(self.next_tag);
        self.next_tag = self.next_tag.wrapping_add(1);
        tag
    }

    /// The core offers to undo `job`: kept when it is one of this
    /// window's, running or offered before.
    fn offered(&mut self, job: JobId) {
        let before = self.undo_gone.len();
        self.undo_gone.retain(|&gone| gone != job);
        let offered_before = self.undo_gone.len() != before;
        if offered_before || self.running.contains_key(&job) {
            self.undoable.push(job);
        }
    }

    /// The core no longer offers to undo `job`; it is kept, the newest
    /// [`UNDO_KEPT`], in case it is offered again.
    fn withdrawn(&mut self, job: JobId) {
        let before = self.undoable.len();
        self.undoable.retain(|&undoable| undoable != job);
        if self.undoable.len() != before {
            self.undo_gone.push(job);
            let over = self.undo_gone.len().saturating_sub(UNDO_KEPT);
            self.undo_gone.drain(..over);
        }
    }
}

impl MainWindow {
    /// Asks the core to run `operation`. Whether the ask was sent.
    pub(crate) fn start_job(&mut self, operation: Operation) -> bool {
        let Some(connection) = &self.connection else {
            return false;
        };
        let tag = self.jobs.next_tag();
        let sent = connection
            .send(Command::StartJob {
                tag,
                operation: operation.clone(),
            })
            .is_ok();
        if sent {
            self.jobs.asked.insert(tag, operation);
        }
        sent
    }

    /// Takes one of the core's job events.
    pub(super) fn job_event(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::JobStarted { tag, job } => {
                if let Some(operation) = self.jobs.asked.remove(&tag) {
                    self.jobs.running.insert(job, operation);
                }
            }
            Event::Undoable { job, .. } => self.jobs.offered(job),
            Event::UndoGone(job) => {
                self.jobs.withdrawn(job);
                if island::drop_undo(&mut self.island.note, job) {
                    cx.notify();
                }
            }
            Event::JobFinished {
                job,
                outcome,
                changed,
            } => self.job_finished(job, &outcome, &changed, cx),
            _ => {}
        }
    }

    fn job_finished(
        &mut self,
        job: JobId,
        outcome: &Outcome,
        changed: &[PathBuf],
        cx: &mut Context<Self>,
    ) {
        self.read_again_if_changed(changed);
        let Some(operation) = self.jobs.running.remove(&job) else {
            return;
        };
        let undo = self
            .jobs
            .undoable
            .contains(&job)
            .then_some(NoteUndo::Job(job));
        if let Some(label) = island::finished_note(&operation, outcome, self.folder()) {
            self.show_note(label, undo, cx);
        }
    }

    /// Reads the folder on screen again when it is among `changed`, unless
    /// the window is already on its way somewhere: that listing will show
    /// the change, or the window leaves the folder.
    fn read_again_if_changed(&mut self, changed: &[PathBuf]) {
        if self.waiting() {
            return;
        }
        let Some(folder) = self.folder().map(super::tidy) else {
            return;
        };
        if changed.iter().any(|path| super::tidy(path) == folder) {
            self.request(&folder);
        }
    }

    /// How many of this window's jobs were asked for and haven't ended.
    pub fn jobs_pending(&self) -> usize {
        self.jobs
            .asked
            .len()
            .saturating_add(self.jobs.running.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_job_asked_for_gets_its_own_tag() {
        let mut jobs = Jobs::default();
        assert_ne!(jobs.next_tag(), jobs.next_tag());
    }

    #[test]
    fn an_undo_withdrawn_comes_back_when_offered_again() {
        let mut jobs = Jobs::default();
        let (mine, other) = (JobId(1), JobId(2));
        jobs.running.insert(mine, Operation::EmptyTrash);
        jobs.offered(mine);
        // Another window's job isn't this window's to undo.
        jobs.offered(other);
        assert_eq!(jobs.undoable, [mine]);

        jobs.running.remove(&mine);
        jobs.withdrawn(other);
        assert_eq!(jobs.undo_gone, []);
        jobs.withdrawn(mine);
        assert_eq!(jobs.last_undoable(), None);
        assert_eq!(jobs.undo_gone, [mine]);
        jobs.offered(mine);
        assert_eq!(jobs.undoable, [mine]);
        assert_eq!(jobs.undo_gone, []);
    }

    #[test]
    fn only_the_newest_withdrawn_undos_are_kept() {
        let mut jobs = Jobs::default();
        let count = u64::try_from(UNDO_KEPT).unwrap_or(u64::MAX) + 1;
        for id in 0..count {
            jobs.running.insert(JobId(id), Operation::EmptyTrash);
            jobs.offered(JobId(id));
            jobs.withdrawn(JobId(id));
        }
        assert_eq!(jobs.undo_gone.len(), UNDO_KEPT);
        assert_eq!(jobs.undo_gone.first(), Some(&JobId(1)));
    }
}
