//! File operations, run as jobs: each on a thread of its own, reported to
//! every client.
//!
//! [`Jobs::start`] gives the job an id, tells the client that asked, and
//! starts the job's thread. When the job ends, every connected client
//! hears how and which folders changed (see `audience`). Nothing a job
//! does ever replaces an existing entry unless the user chose that.

mod audience;
mod failure;
mod name;
mod one_step;
mod place;
#[cfg(test)]
mod test_folder;

use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

use nova_files_api::{ClientGone, Event, Failure, JobId, JobTag, Operation, Outcome};

pub use audience::Audience;

use crate::jobs::Client;
use one_step::Ended;

const JOB_THREAD: &str = "nova-files-job";

/// The core's jobs.
#[derive(Default)]
pub struct Jobs {
    next: AtomicU64,
    audience: Audience,
}

impl Jobs {
    /// The clients that hear about jobs.
    pub const fn audience(&self) -> &Audience {
        &self.audience
    }

    /// Starts `operation` for `client`, which tagged it `tag`.
    ///
    /// # Errors
    /// [`ClientGone`] when `client` stopped listening. The job runs anyway.
    pub fn start(
        &self,
        client: &Client,
        tag: JobTag,
        operation: Operation,
    ) -> Result<(), ClientGone> {
        let job = JobId(self.next.fetch_add(1, Ordering::Relaxed));
        // Before the thread starts, so the client hears of the job before
        // it can end.
        let told = client.deliver(Event::JobStarted { tag, job });
        let audience = self.audience.clone();
        let failed_path = match &operation {
            Operation::Rename { path, .. } => path.clone(),
            Operation::NewFolder { inside, name } => inside.join(name),
        };
        let spawned = thread::Builder::new()
            .name(JOB_THREAD.to_owned())
            .spawn(move || finish(&audience, job, run(&operation)));
        if spawned.is_err() {
            let outcome = Outcome::Failed {
                path: failed_path,
                reason: Failure::Other,
            };
            finish(&self.audience, job, (outcome, Vec::new()));
        }
        told
    }
}

fn run(operation: &Operation) -> Ended {
    match operation {
        Operation::Rename { path, name } => one_step::rename(path, name),
        Operation::NewFolder { inside, name } => one_step::new_folder(inside, name),
    }
}

fn finish(audience: &Audience, job: JobId, (outcome, changed): Ended) {
    audience.tell(&Event::JobFinished {
        job,
        outcome,
        changed,
    });
}
