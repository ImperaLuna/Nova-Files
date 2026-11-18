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

    pub fn progress(&self) -> Progress {
        *self.progress.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn set_progress(&self, progress: Progress) {
        *self.progress.lock().unwrap_or_else(PoisonError::into_inner) = progress;
    }
}
