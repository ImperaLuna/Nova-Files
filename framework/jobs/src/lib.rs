#![forbid(unsafe_code)]

pub mod audience;
pub mod clash;
pub mod control;
pub mod failure;
pub mod folders;
pub mod history;
pub mod job;
pub mod journal;
pub mod left;
pub mod mover;
pub mod place;
pub mod plan;
pub mod progress;
pub mod running;
pub mod set_aside;
pub mod stop;
pub mod sync;
#[cfg(any(test, feature = "test-support"))]
pub mod test_folder;
pub mod trash_info;

mod runner;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use nova_files_api::JobId;

pub use runner::{Ended, JobKind, Jobs};

pub type Registry = Arc<Mutex<HashMap<JobId, Arc<job::Job>>>>;

pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
