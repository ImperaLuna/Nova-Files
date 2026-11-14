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
}

impl MainWindow {
    /// Asks the core to run `operation`. Whether the ask was sent.
    pub(crate) fn start_job(&mut self, operation: Operation) -> bool {
        let Some(connection) = &self.connection else {
            return false;
        };
        let tag = JobTag(self.jobs.next_tag);
        self.jobs.next_tag += 1;
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
            Event::Undoable { job, .. } => {
                let gone = self.jobs.undo_gone.iter().position(|&gone| gone == job);
                if let Some(gone) = gone {
                    self.jobs.undo_gone.remove(gone);
                }
                if gone.is_some() || self.jobs.running.contains_key(&job) {
                    self.jobs.undoable.push(job);
                }
            }
            Event::UndoGone(job) => {
                let before = self.jobs.undoable.len();
                self.jobs.undoable.retain(|&undoable| undoable != job);
                if self.jobs.undoable.len() < before {
                    self.jobs.undo_gone.push(job);
                    let over = self.jobs.undo_gone.len().saturating_sub(UNDO_KEPT);
                    self.jobs.undo_gone.drain(..over);
                }
                let named = |note: &island::Note| note.undo == Some(NoteUndo::Job(job));
                if self.island.note.as_ref().is_some_and(named) {
                    if let Some(note) = &mut self.island.note {
                        note.undo = None;
                    }
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

    /// The core has stopped: no job of this window will say more.
    pub(super) fn jobs_gone(&mut self) {
        self.jobs = Jobs::default();
    }
}
