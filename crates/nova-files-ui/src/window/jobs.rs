//! The jobs this window starts, and what the core says about every job.
//!
//! A job starts with a tag the window picks; the core's `JobStarted`
//! names the job that tag became, and from then on the window follows it
//! by id. When any job ends, a folder it changed that is on screen is read
//! again, so what changed shows. When one of this window's own jobs ends,
//! the island says how it went, with Undo when the core said the job can
//! be undone (see `island`). Jobs other windows started are only read
//! for what they changed.
//!
//! While one of its jobs runs, the island shows the newest one's progress
//! and time left, once it has run for [`SHOW_AFTER`], so a quick job only
//! leaves its note. A copy or move into the folder on screen reads the
//! folder again as files finish, at most every [`READ_AGAIN_EVERY`], so
//! they show as they land. A copy onto itself keeps both, as a paste into
//! the folder the files came from should; any other conflict shows the
//! job as waiting, with Cancel: what the island asks about a conflict
//! isn't decided yet.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{App, Context, Task};
use nova_files_api::{
    Answer, Command, Conflict, ConflictKind, Event, JobId, JobTag, Operation, Outcome, Progress,
    Scope,
};

use super::MainWindow;
use crate::island::progress::Running;
use crate::island::{self, NoteUndo};

/// How long a job runs before the island shows its progress.
pub const SHOW_AFTER: Duration = Duration::from_millis(400);
/// How often a copy or move into the folder on screen reads it again.
pub const READ_AGAIN_EVERY: Duration = Duration::from_millis(500);

/// What the window knows about the jobs it started.
#[derive(Debug, Default)]
pub struct Jobs {
    next_tag: u64,
    /// Jobs asked for, by tag, until the core says which job each is.
    asked: HashMap<JobTag, Operation>,
    /// This window's jobs while they run.
    running: HashMap<JobId, Running>,
    /// This window's jobs that can still be undone, oldest first.
    undoable: Vec<JobId>,
    /// This window's finished jobs the core stopped offering to undo, the
    /// newest last: an undo that fails or is cancelled offers its job
    /// again. At most [`UNDO_KEPT`].
    undo_gone: Vec<JobId>,
    /// Redraws the window once the newest job has run for [`SHOW_AFTER`].
    show_timer: Option<Task<()>>,
    /// When a job last read the folder on screen again.
    read_again: Option<Instant>,
}

/// How many jobs the core keeps to undo; the window keeps as many it
/// may offer again.
const UNDO_KEPT: usize = 50;

impl Jobs {
    /// The job the island shows at `now`: the newest of this window's
    /// running jobs that has run for [`SHOW_AFTER`].
    pub fn on_island(&self, now: Instant) -> Option<(JobId, &Running)> {
        self.running
            .iter()
            .filter(|(_, job)| now.saturating_duration_since(job.started) >= SHOW_AFTER)
            .max_by_key(|(id, job)| (job.started, **id))
            .map(|(id, job)| (*id, job))
    }

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
                    let now = cx.background_executor().now();
                    self.jobs.running.insert(job, Running::new(operation, now));
                    self.jobs.show_timer = Some(cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(SHOW_AFTER).await;
                        this.update(cx, |_, cx| cx.notify()).ok();
                    }));
                }
            }
            Event::JobProgress { job, progress } => self.job_progressed(job, progress, cx),
            Event::JobConflict { job, conflict } => self.job_conflict(job, conflict, cx),
            Event::ConflictAnswered { job, .. } => {
                self.job_held(job, |job| job.waiting = false, cx);
            }
            Event::JobPaused(job) => self.job_held(job, |job| job.paused = true, cx),
            Event::JobResumed(job) => self.job_held(job, |job| job.paused = false, cx),
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
        let Some(Running { operation, .. }) = self.jobs.running.remove(&job) else {
            return;
        };
        cx.notify();
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
    /// the change, or the window leaves the folder. Whether it was asked
    /// for.
    fn read_again_if_changed(&mut self, changed: &[PathBuf]) -> bool {
        if self.waiting() {
            return false;
        }
        let Some(folder) = self.folder().map(super::tidy) else {
            return false;
        };
        if changed.iter().any(|path| super::tidy(path) == folder) {
            self.request(&folder)
        } else {
            false
        }
    }

    /// Takes the progress of job `job`; one of this window's copies or
    /// moves into the folder on screen reads it again as files finish.
    fn job_progressed(&mut self, job: JobId, progress: Progress, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        let Some(running) = self.jobs.running.get_mut(&job) else {
            return;
        };
        let landed = progress.done_items > running.progress.done_items;
        running.advanced(progress, now);
        let into = match &running.operation {
            Operation::Copy { into, .. } | Operation::Move { into, .. } => Some(into.clone()),
            _ => None,
        };
        let due = self
            .jobs
            .read_again
            .is_none_or(|last| now.saturating_duration_since(last) >= READ_AGAIN_EVERY);
        if landed
            && due
            && let Some(into) = into
            && self.read_again_if_changed(&[into])
        {
            self.jobs.read_again = Some(now);
        }
        cx.notify();
    }

    /// Job `job` holds at `conflict`. A copy of a file onto itself (a
    /// paste into the folder it came from) keeps both, as a copy in place
    /// should; anything else waits, since what the island asks isn't
    /// decided yet.
    fn job_conflict(&mut self, job: JobId, conflict: Conflict, cx: &mut Context<Self>) {
        let copies = self
            .jobs
            .running
            .get(&job)
            .is_some_and(|running| matches!(running.operation, Operation::Copy { .. }));
        if copies
            && conflict.kind == ConflictKind::SameFile
            && let Some(connection) = &self.connection
        {
            connection
                .send(Command::AnswerConflict {
                    job,
                    conflict,
                    answer: Answer::KeepBoth,
                    scope: Scope::AllOfThisKind,
                })
                .ok();
            return;
        }
        self.job_held(job, |job| job.waiting = true, cx);
    }

    /// Changes how job `job` holds, if it is this window's.
    fn job_held(&mut self, job: JobId, change: impl FnOnce(&mut Running), cx: &mut Context<Self>) {
        if let Some(running) = self.jobs.running.get_mut(&job) {
            change(running);
            cx.notify();
        }
    }

    /// The line of this window's newest running job, shown or not yet.
    pub fn newest_job_line(&self, cx: &App) -> Option<String> {
        let now = cx.background_executor().now();
        self.jobs
            .running
            .values()
            .max_by_key(|job| job.started)
            .map(|job| job.line(now))
    }

    /// The job the island shows now, if any.
    pub(crate) fn job_on_island(&self, cx: &Context<Self>) -> Option<(JobId, &Running)> {
        self.jobs.on_island(cx.background_executor().now())
    }

    /// Cancels the job the island shows.
    pub(crate) fn cancel_job(&self, cx: &Context<Self>) {
        let shown = self.job_on_island(cx).map(|(job, _)| job);
        if let (Some(job), Some(connection)) = (shown, &self.connection) {
            connection.send(Command::CancelJob(job)).ok();
        }
    }

    /// The core has stopped: no job of this window will say more.
    pub(super) fn jobs_gone(&mut self) {
        self.jobs = Jobs::default();
    }
}
