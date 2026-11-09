//! One job as the core keeps it while it runs: what it does, its switch
//! (see `control`) and how far it last said it got, so a window that
//! starts watching jobs later can be told.

use std::sync::{Mutex, PoisonError};

use nova_files_api::{Operation, Progress};

use super::control::Control;

pub struct Job {
    pub operation: Operation,
    pub control: Control,
    progress: Mutex<Progress>,
}

impl Job {
    pub fn new(operation: Operation) -> Self {
        Self {
            operation,
            control: Control::default(),
            progress: Mutex::default(),
        }
    }

    /// How far the job last said it got.
    pub fn progress(&self) -> Progress {
        *self.progress.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn set_progress(&self, progress: Progress) {
        *self.progress.lock().unwrap_or_else(PoisonError::into_inner) = progress;
    }
}
