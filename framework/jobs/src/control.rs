use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

use nova_files_api::{Answer, Conflict, ConflictKind, Scope};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wanted {
    Run,
    Pause,
    Cancel,
}

struct State {
    wanted: Wanted,
    open: Option<Conflict>,
    answer: Option<(Answer, Scope)>,
    password: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

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
                password: None,
            }),
            changed: Condvar::new(),
        }
    }
}

impl Control {
    pub fn pause(&self) -> bool {
        self.set(Wanted::Run, Wanted::Pause)
    }

    pub fn resume(&self) -> bool {
        self.set(Wanted::Pause, Wanted::Run)
    }

    pub fn paused(&self) -> bool {
        self.lock().wanted == Wanted::Pause
    }

    pub fn cancel(&self) {
        self.lock().wanted = Wanted::Cancel;
        self.changed.notify_all();
    }

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

    pub fn raise(&self, conflict: Conflict) {
        let mut state = self.lock();
        state.open = Some(conflict);
        state.answer = None;
    }

    pub fn open(&self) -> Option<Conflict> {
        self.lock().open.clone()
    }

    pub fn answer(
        &self,
        conflict: &Conflict,
        answer: Answer,
        scope: Scope,
        heard: impl FnOnce(),
    ) -> bool {
        self.settle(conflict, (answer, scope), None, heard)
    }

    pub fn give_password(
        &self,
        conflict: &Conflict,
        password: String,
        heard: impl FnOnce(),
    ) -> bool {
        matches!(
            conflict.kind,
            ConflictKind::PasswordNeeded | ConflictKind::PasswordWrong
        ) && self.settle(
            conflict,
            (Answer::Retry, Scope::ThisOne),
            Some(password),
            heard,
        )
    }

    #[expect(
        clippy::significant_drop_tightening,
        reason = "telling under the lock keeps the job from going on unheard"
    )]
    fn settle(
        &self,
        conflict: &Conflict,
        (answer, scope): (Answer, Scope),
        password: Option<String>,
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
                state.password = password;
                heard();
            }
            takes
        };
        self.changed.notify_all();
        taken
    }

    pub fn take_password(&self) -> Option<String> {
        self.lock().password.take()
    }

    pub fn cancelled(&self) -> bool {
        self.lock().wanted == Wanted::Cancel
    }

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
