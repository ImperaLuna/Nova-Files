//! Pausing and cancelling a running job.
//!
//! The core's loop sets what a client wants; the job's thread calls
//! [`Control::checkpoint`] before each change to the disk, which waits
//! there while the job is paused and stops it once cancelled. A paused job
//! waits without a deadline: it holds only until a client resumes or
//! cancels it, which may be much later.

use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

/// What clients last asked of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wanted {
    Run,
    Pause,
    Cancel,
}

/// A job was cancelled; returned by [`Control::checkpoint`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

/// One job's switch, shared by the core's loop and the job's thread.
pub struct Control {
    wanted: Mutex<Wanted>,
    changed: Condvar,
}

impl Default for Control {
    fn default() -> Self {
        Self {
            wanted: Mutex::new(Wanted::Run),
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
        *self.lock() == Wanted::Pause
    }

    /// Stops the job before its next change, held or not.
    pub fn cancel(&self) {
        *self.lock() = Wanted::Cancel;
        self.changed.notify_all();
    }

    /// Returns at once while the job runs, waits while it is held, and
    /// fails once it is cancelled.
    ///
    /// # Errors
    /// [`Cancelled`] when a client cancelled the job.
    pub fn checkpoint(&self) -> Result<(), Cancelled> {
        let wanted = self
            .changed
            .wait_while(self.lock(), |wanted| matches!(wanted, Wanted::Pause))
            .unwrap_or_else(PoisonError::into_inner);
        match *wanted {
            Wanted::Cancel => Err(Cancelled),
            Wanted::Run | Wanted::Pause => Ok(()),
        }
    }

    /// Moves from `from` to `to`, and only from `from`.
    fn set(&self, from: Wanted, to: Wanted) -> bool {
        {
            let mut wanted = self.lock();
            if *wanted != from {
                return false;
            }
            *wanted = to;
        }
        self.changed.notify_all();
        true
    }

    fn lock(&self) -> MutexGuard<'_, Wanted> {
        self.wanted.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use super::*;

    const SHORT: Duration = Duration::from_millis(50);
    const LONG: Duration = Duration::from_secs(5);

    #[test]
    fn runs_until_cancelled() {
        let control = Control::default();
        assert_eq!(control.checkpoint(), Ok(()));
        control.cancel();
        assert_eq!(control.checkpoint(), Err(Cancelled));
        assert!(!control.pause());
        assert!(!control.resume());
    }

    #[test]
    fn a_held_job_waits_until_resumed() {
        let control = Arc::new(Control::default());
        assert!(!control.paused());
        assert!(control.pause());
        assert!(control.paused());
        assert!(!control.pause());
        let (passed, reached) = mpsc::channel();
        let job = Arc::clone(&control);
        thread::spawn(move || passed.send(job.checkpoint()).unwrap());

        assert!(reached.recv_timeout(SHORT).is_err());
        assert!(control.resume());
        assert_eq!(reached.recv_timeout(LONG), Ok(Ok(())));
        assert!(!control.resume());
    }

    #[test]
    fn a_held_job_can_be_cancelled() {
        let control = Arc::new(Control::default());
        control.pause();
        let (passed, reached) = mpsc::channel();
        let job = Arc::clone(&control);
        thread::spawn(move || passed.send(job.checkpoint()).unwrap());

        assert!(reached.recv_timeout(SHORT).is_err());
        control.cancel();
        assert_eq!(reached.recv_timeout(LONG), Ok(Err(Cancelled)));
    }
}
