//! Pausing, cancelling and answering a running job.
//!
//! The core's loop sets what a client wants; the job's thread calls
//! [`Control::checkpoint`] before each change to the disk, which waits
//! there while the job is paused and stops it once cancelled. At a
//! conflict the thread raises it ([`Control::raise`]) and waits for an
//! answer ([`Control::answer_to`]); a client's answer counts only when the
//! conflict's kind accepts it. Waits have no deadline: a job holds until a
//! client resumes, answers or cancels it, which may be much later.

use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

use nova_files_api::{Answer, Conflict, Scope};

/// What clients last asked of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wanted {
    Run,
    Pause,
    Cancel,
}

struct State {
    wanted: Wanted,
    /// The conflict the job holds at, until answered.
    open: Option<Conflict>,
    answer: Option<(Answer, Scope)>,
}

/// A job was cancelled; returned by [`Control::checkpoint`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

/// One job's switch, shared by the core's loop and the job's thread.
pub struct Control {
    state: Mutex<State>,
    changed: Condvar,
}

impl Default for Control {
    fn default() -> Self {
        Self {
            state: Mutex::new(State {
                wanted: Wanted::Run,
                open: None,
                answer: None,
            }),
            changed: Condvar::new(),
        }
    }
}

impl Control {
    /// Holds the job before its next change; `false` when it was already
    /// held or cancelled, so nothing changed.
    pub fn pause(&self) -> bool {
        self.set(Wanted::Run, Wanted::Pause)
    }

    /// Lets a held job go on; `false` when it wasn't held.
    pub fn resume(&self) -> bool {
        self.set(Wanted::Pause, Wanted::Run)
    }

    /// Whether the job is held.
    pub fn paused(&self) -> bool {
        self.lock().wanted == Wanted::Pause
    }

    /// Stops the job before its next change, held or not, and wakes it
    /// from a conflict.
    pub fn cancel(&self) {
        self.lock().wanted = Wanted::Cancel;
        self.changed.notify_all();
    }

    /// Returns at once while the job runs, waits while it is held, and
    /// fails once it is cancelled.
    ///
    /// # Errors
    /// [`Cancelled`] when a client cancelled the job.
    pub fn checkpoint(&self) -> Result<(), Cancelled> {
        let state = self
            .changed
            .wait_while(self.lock(), |state| matches!(state.wanted, Wanted::Pause))
            .unwrap_or_else(PoisonError::into_inner);
        match state.wanted {
            Wanted::Cancel => Err(Cancelled),
            Wanted::Run | Wanted::Pause => Ok(()),
        }
    }

    /// The job holds at `conflict` from now on, until answered.
    pub fn raise(&self, conflict: Conflict) {
        let mut state = self.lock();
        state.open = Some(conflict);
        state.answer = None;
    }

    /// The conflict the job holds at, if any.
    pub fn open(&self) -> Option<Conflict> {
        self.lock().open.clone()
    }

    /// Answers `conflict`, calling `heard` before the job can go on, so
    /// the answer is told before any news of what follows; `false` when
    /// it isn't the one open, or its kind doesn't take `answer`.
    #[expect(
        clippy::significant_drop_tightening,
        reason = "telling under the lock keeps the job from going on unheard"
    )]
    pub fn answer(
        &self,
        conflict: &Conflict,
        answer: Answer,
        scope: Scope,
        heard: impl FnOnce(),
    ) -> bool {
        let taken = {
            let mut state = self.lock();
            let takes = state
                .open
                .as_ref()
                .is_some_and(|open| open == conflict && open.kind.accepts(answer));
            if takes {
                state.open = None;
                state.answer = Some((answer, scope));
                heard();
            }
            takes
        };
        self.changed.notify_all();
        taken
    }

    /// Waits for the answer to the conflict raised last.
    ///
    /// # Errors
    /// [`Cancelled`] when a client cancelled the job instead.
    pub fn answer_to(&self) -> Result<(Answer, Scope), Cancelled> {
        let mut state = self
            .changed
            .wait_while(self.lock(), |state| {
                matches!(
                    (&state.answer, state.wanted),
                    (None, Wanted::Run | Wanted::Pause)
                )
            })
            .unwrap_or_else(PoisonError::into_inner);
        state.open = None;
        state.answer.take().ok_or(Cancelled)
    }

    /// Moves from `from` to `to`, and only from `from`.
    fn set(&self, from: Wanted, to: Wanted) -> bool {
        {
            let mut state = self.lock();
            if state.wanted != from {
                return false;
            }
            state.wanted = to;
        }
        self.changed.notify_all();
        true
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests;
