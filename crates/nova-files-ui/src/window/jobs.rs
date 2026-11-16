//! The jobs this window starts, and what the core says about every job.
//!
//! A job starts with a tag the window picks; the core's `JobStarted`
//! names the job that tag became and what it does (a drop of files is a
//! copy or a move as the core finds), and from then on the window follows
//! it by id. When any job ends, a folder it changed that is on screen is read
//! again, so what changed shows. When one of this window's own jobs ends,
//! the island says how it went, with Undo when the core said the job can
//! be undone (see `island`). Jobs other windows started are only read
//! for what they changed.
//!
//! While one of its jobs runs, the island shows its progress and time
//! left, once it has run for [`SHOW_AFTER`], so a quick job only leaves
//! its note; several show folded into one line that opens into a row each
//! (see `island::several`), and copies and moves that end while others
//! run leave one note when the last ends. A copy or move into the folder on screen reads the
//! folder again as files finish, at most every [`READ_AGAIN_EVERY`], so
//! they show as they land. A copy onto itself keeps both, as a paste into
//! the folder the files came from should; at any other conflict the job
//! holds and the island asks about it at once (see `island::asking`),
//! before any other job's progress.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{App, Context, Task};
use nova_files_api::{
    Answer, Command, Conflict, ConflictKind, Event, JobId, JobTag, Operation, Outcome, Scope, Tally,
};

use super::MainWindow;
use crate::island::progress::Running;
use crate::island::several::Batch;
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
    asked: HashSet<JobTag>,
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
    /// Copies and moves that ended while others still ran.
    batch: Batch,
}

/// How many jobs the core keeps to undo; the window keeps as many it
/// may offer again.
const UNDO_KEPT: usize = 50;

impl Jobs {
    /// The job the island shows at `now`: the oldest of this window's
    /// jobs that holds at a conflict, else the newest that has run for
    /// [`SHOW_AFTER`].
    pub fn on_island(&self, now: Instant) -> Option<(JobId, &Running)> {
        let asking = self
            .running
            .iter()
            .filter(|(_, job)| job.asking.is_some())
            .min_by_key(|(id, job)| (job.started, **id));
        asking
            .or_else(|| {
                self.running
                    .iter()
                    .filter(|(_, job)| now.saturating_duration_since(job.started) >= SHOW_AFTER)
                    .max_by_key(|(id, job)| (job.started, **id))
            })
            .map(|(id, job)| (*id, job))
    }

    /// This window's jobs that have run for [`SHOW_AFTER`] at `now`,
    /// oldest first: the rows of the island's list.
    pub fn shown(&self, now: Instant) -> Vec<(JobId, &Running)> {
        let mut shown: Vec<_> = self
            .running
            .iter()
            .filter(|(_, job)| now.saturating_duration_since(job.started) >= SHOW_AFTER)
            .map(|(id, job)| (*id, job))
            .collect();
        shown.sort_by_key(|(id, job)| (job.started, *id));
        shown
    }

    /// The newest of this window's jobs that can still be undone.
    pub fn last_undoable(&self) -> Option<JobId> {
        self.undoable.last().copied()
    }

    /// Takes how one of this window's jobs moves on, at `now`: its
    /// progress, or its holding for a conflict or a pause. The folder a
    /// copy or move goes into when files landed in it and it is due to be
    /// read again (see [`READ_AGAIN_EVERY`]).
    pub(super) fn moved(&mut self, event: &Event, now: Instant) -> Option<PathBuf> {
        let (job, change): (_, fn(&mut Running)) = match *event {
            Event::JobProgress { job, progress } => {
                let running = self.running.get_mut(&job)?;
                let landed = progress.done_items > running.progress.done_items;
                running.advanced(progress, now);
                let (Operation::Copy { into, .. } | Operation::Move { into, .. }) =
                    &running.operation
                else {
                    return None;
                };
                let due = self
                    .read_again
                    .is_none_or(|last| now.saturating_duration_since(last) >= READ_AGAIN_EVERY);
                return (landed && due).then(|| into.clone());
            }
            Event::ConflictAnswered { job, .. } => (job, |job| job.asking = None),
            Event::JobPaused(job) => (job, |job| job.paused = true),
            Event::JobResumed(job) => (job, |job| job.paused = false),
            _ => return None,
        };
        if let Some(running) = self.running.get_mut(&job) {
            change(running);
        }
        None
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
        self.ask_job(|tag| Command::StartJob { tag, operation })
    }

    /// Asks the core to copy or move `sources` into `into` as a drop
    /// does, by drive. Whether the ask was sent.
    pub(crate) fn start_drop(&mut self, sources: Vec<PathBuf>, into: PathBuf) -> bool {
        self.ask_job(|tag| Command::DropFiles { tag, sources, into })
    }

    /// Sends the command `command` makes of a new job tag. Whether it was
    /// sent.
    fn ask_job(&mut self, command: impl FnOnce(JobTag) -> Command) -> bool {
        let Some(connection) = &self.connection else {
            return false;
        };
        let tag = self.jobs.next_tag();
        let sent = connection.send(command(tag)).is_ok();
        if sent {
            self.jobs.asked.insert(tag);
        }
        sent
    }

    /// Takes how a job moves on (see [`Jobs::moved`]), reading the folder
    /// on screen again when files landed in it.
    pub(super) fn job_moved(&mut self, event: &Event, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        if let Some(into) = self.jobs.moved(event, now)
            && self.read_again_if_changed(&[into])
        {
            self.jobs.read_again = Some(now);
        }
        cx.notify();
    }

    /// Takes one of the core's job events.
    pub(super) fn job_event(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::JobStarted {
                tag,
                job,
                operation,
            } => {
                if self.jobs.asked.remove(&tag) {
                    let now = cx.background_executor().now();
                    self.jobs.running.insert(job, Running::new(operation, now));
                    self.jobs.show_timer = Some(cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(SHOW_AFTER).await;
                        this.update(cx, |_, cx| cx.notify()).ok();
                    }));
                }
            }
            Event::JobConflict { job, conflict } => self.job_conflict(job, conflict, cx),
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
                tally,
            } => self.job_finished(job, (&outcome, tally), &changed, cx),
            _ => {}
        }
    }

    fn job_finished(
        &mut self,
        job: JobId,
        (outcome, tally): (&Outcome, Tally),
        changed: &[PathBuf],
        cx: &mut Context<Self>,
    ) {
        self.read_again_if_changed(changed);
        let Some(Running { operation, .. }) = self.jobs.running.remove(&job) else {
            return;
        };
        cx.notify();
        if *outcome != Outcome::Done
            && matches!(
                operation,
                Operation::Rename { .. } | Operation::NewFolder { .. }
            )
        {
            // Nothing was made to select.
            self.waiting = super::renaming::Waiting::Nothing;
        }
        self.trash_maybe_changed(&operation);
        let transfer = is_transfer(&operation);
        let others = self.transfers_running();
        let batching = others || !self.jobs.batch.is_empty();
        // A copy or move that ends while others run waits for the last to
        // say what they all did.
        if transfer && batching && *outcome == Outcome::Done {
            self.jobs.batch.keep(&operation, tally);
            if !others {
                let label = self.jobs.batch.note();
                self.show_note(label, None, cx);
            }
            return;
        }
        let undo = self
            .jobs
            .undoable
            .contains(&job)
            .then_some(NoteUndo::Job(job));
        let own = island::finished_note(&operation, outcome, self.folder(), tally);
        // The last of several stopped or failed: its failure says more
        // than what the others did.
        let together = (transfer && !others && !self.jobs.batch.is_empty())
            .then(|| self.jobs.batch.note())
            .filter(|_| own.is_none());
        if let Some(label) = own {
            self.show_note(label, undo, cx);
        } else if let Some(label) = together {
            self.show_note(label, None, cx);
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

    /// Job `job` holds at `conflict`. A copy of a file onto itself (a
    /// paste into the folder it came from) keeps both, as a copy in place
    /// should; the island asks about anything else, its "For all" tick
    /// clear.
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
        if let Some(running) = self.jobs.running.get_mut(&job) {
            running.asking = Some(conflict);
            self.island.for_all = false;
            cx.notify();
        }
    }

    /// Answers the conflict the island asks about with `answer`, for it
    /// alone or, with the "For all" tick, for every one like it.
    pub(crate) fn answer_conflict(&mut self, answer: Answer, cx: &mut Context<Self>) {
        let asked = self
            .job_on_island(cx)
            .and_then(|(job, running)| Some((job, running.asking.clone()?)));
        let (Some((job, conflict)), Some(connection)) = (asked, &self.connection) else {
            return;
        };
        let scope = if self.island.for_all && conflict.more > 0 {
            Scope::AllOfThisKind
        } else {
            Scope::ThisOne
        };
        connection
            .send(Command::AnswerConflict {
                job,
                conflict,
                answer,
                scope,
            })
            .ok();
        self.island.for_all = false;
        cx.notify();
    }

    /// Cancels the job the island shows: what it already did stays done.
    pub(crate) fn cancel_job(&self, cx: &Context<Self>) {
        let shown = self.job_on_island(cx).map(|(job, _)| job);
        if let (Some(job), Some(connection)) = (shown, &self.connection) {
            connection.send(Command::CancelJob(job)).ok();
        }
    }

    /// Ticks or clears "For all" on the island's question.
    pub(crate) fn toggle_for_all(&mut self, cx: &mut Context<Self>) {
        self.island.for_all = !self.island.for_all;
        cx.notify();
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

    /// This window's jobs on the island now, oldest first.
    pub(crate) fn jobs_shown(&self, cx: &Context<Self>) -> Vec<(JobId, &Running)> {
        self.jobs.shown(cx.background_executor().now())
    }

    /// Whether any of this window's copies or moves still runs.
    fn transfers_running(&self) -> bool {
        self.jobs
            .running
            .values()
            .any(|job| is_transfer(&job.operation))
    }

    /// Stops job `job`, one of the island's rows.
    pub(crate) fn stop_job(&self, job: JobId) {
        if let Some(connection) = &self.connection {
            connection.send(Command::CancelJob(job)).ok();
        }
    }

    /// Opens or folds the island's list of jobs.
    pub(crate) fn toggle_jobs_open(&mut self, cx: &mut Context<Self>) {
        self.island.jobs_open = !self.island.jobs_open;
        cx.notify();
    }

    /// How many of this window's jobs were asked for and haven't ended.
    pub fn jobs_pending(&self) -> usize {
        self.jobs
            .asked
            .len()
            .saturating_add(self.jobs.running.len())
    }
}

/// Whether `operation` copies or moves.
const fn is_transfer(operation: &Operation) -> bool {
    matches!(operation, Operation::Copy { .. } | Operation::Move { .. })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use nova_files_api::Progress;

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
        jobs.running
            .insert(mine, Running::new(Operation::EmptyTrash, Instant::now()));
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
            let running = Running::new(Operation::EmptyTrash, Instant::now());
            jobs.running.insert(JobId(id), running);
            jobs.offered(JobId(id));
            jobs.withdrawn(JobId(id));
        }
        assert_eq!(jobs.undo_gone.len(), UNDO_KEPT);
        assert_eq!(jobs.undo_gone.first(), Some(&JobId(1)));
    }

    fn items(done_items: u64) -> Progress {
        Progress {
            done_items,
            total_items: 3,
            ..Progress::default()
        }
    }

    fn progress(job: JobId, done: u64) -> Event {
        Event::JobProgress {
            job,
            progress: items(done),
        }
    }

    #[test]
    fn a_copy_reads_its_folder_again_as_files_land_but_not_too_often() {
        let start = Instant::now();
        let mut jobs = Jobs::default();
        let job = JobId(1);
        let into = PathBuf::from("/b");
        let copy = Operation::Copy {
            sources: vec![PathBuf::from("/a/x")],
            into: into.clone(),
        };
        jobs.running.insert(job, Running::new(copy, start));
        // Nothing landed yet.
        assert_eq!(jobs.moved(&progress(job, 0), start), None);
        assert_eq!(jobs.moved(&progress(job, 1), start), Some(into.clone()));
        jobs.read_again = Some(start);
        let soon = start + READ_AGAIN_EVERY / 2;
        assert_eq!(jobs.moved(&progress(job, 2), soon), None);
        // Nothing more landed.
        let later = start + READ_AGAIN_EVERY;
        assert_eq!(jobs.moved(&progress(job, 2), later), None);
        assert_eq!(jobs.moved(&progress(job, 3), later), Some(into));
        assert_eq!(jobs.running[&job].progress, items(3));
        // Another window's job.
        assert_eq!(jobs.moved(&progress(JobId(2), 3), later), None);
    }

    #[test]
    fn other_jobs_read_nothing_again() {
        let start = Instant::now();
        let mut jobs = Jobs::default();
        let trash = Operation::Trash {
            paths: vec![PathBuf::from("/a/x")],
        };
        jobs.running.insert(JobId(1), Running::new(trash, start));
        let later = start + Duration::from_secs(1);
        assert_eq!(jobs.moved(&progress(JobId(1), 1), later), None);
        assert_eq!(jobs.running[&JobId(1)].progress, items(1));
    }

    #[test]
    fn a_job_holds_while_paused_or_waiting() {
        let start = Instant::now();
        let mut jobs = Jobs::default();
        let job = JobId(1);
        let empty = Operation::EmptyTrash;
        jobs.running.insert(job, Running::new(empty, start));
        jobs.running.get_mut(&job).unwrap().asking = Some(Conflict {
            kind: ConflictKind::FileExists,
            source: PathBuf::from("/a/x"),
            target: PathBuf::from("/b/x"),
            number: 1,
            more: 0,
        });
        jobs.moved(&Event::JobPaused(job), start);
        assert!(jobs.running[&job].paused);
        jobs.moved(&Event::JobResumed(job), start);
        assert!(!jobs.running[&job].paused);
        let answer = Event::ConflictAnswered {
            job,
            answer: Answer::KeepBoth,
        };
        jobs.moved(&answer, start);
        assert!(jobs.running[&job].asking.is_none());
        // Another window's job.
        assert_eq!(jobs.moved(&Event::JobPaused(JobId(2)), start), None);
    }

    #[test]
    fn a_job_asking_shows_at_once_before_newer_ones() {
        let start = Instant::now();
        let mut jobs = Jobs::default();
        let shown = start + SHOW_AFTER;
        assert!(jobs.on_island(shown).is_none());
        jobs.running
            .insert(JobId(1), Running::new(Operation::EmptyTrash, start));
        jobs.running.insert(
            JobId(2),
            Running::new(Operation::EmptyTrash, start + SHOW_AFTER / 2),
        );
        jobs.running
            .insert(JobId(3), Running::new(Operation::EmptyTrash, shown));
        // Only those that ran long enough, the newest first.
        assert_eq!(jobs.on_island(shown).map(|(id, _)| id), Some(JobId(1)));
        assert_eq!(
            jobs.on_island(shown + SHOW_AFTER).map(|(id, _)| id),
            Some(JobId(3))
        );
        let clash = Conflict {
            kind: ConflictKind::FileExists,
            source: PathBuf::from("/a/x"),
            target: PathBuf::from("/b/x"),
            number: 1,
            more: 0,
        };
        for id in [JobId(2), JobId(3)] {
            jobs.running.get_mut(&id).unwrap().asking = Some(clash.clone());
        }
        // The oldest asking, however new.
        assert_eq!(jobs.on_island(start).map(|(id, _)| id), Some(JobId(2)));
    }
}
