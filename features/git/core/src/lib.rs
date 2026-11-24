#![forbid(unsafe_code)]

mod status;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use nova_files_api::{ClientId, EventSink};
use nova_files_git_api::Event;

pub use status::repo_at;

const THREAD: &str = "nova-files-git";

#[derive(Default)]
pub struct Reader {
    reading: Mutex<HashMap<ClientId, Arc<AtomicBool>>>,
}

impl Reader {
    pub fn read(&self, client: ClientId, events: &Arc<dyn EventSink>, folders: Vec<PathBuf>) {
        let stop = Arc::new(AtomicBool::new(false));
        let earlier = self.lock().insert(client, Arc::clone(&stop));
        if let Some(earlier) = earlier {
            earlier.store(true, Ordering::Relaxed);
        }
        let events = Arc::clone(events);
        let started = thread::Builder::new()
            .name(THREAD.to_owned())
            .spawn(move || read_each(&events, folders, &stop));
        if let Err(error) = started {
            log::error!("no thread to read git state: {error}");
        }
    }

    pub fn forget(&self, client: ClientId) {
        let reading = self.lock().remove(&client);
        if let Some(reading) = reading {
            reading.store(true, Ordering::Relaxed);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<ClientId, Arc<AtomicBool>>> {
        self.reading.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn read_each(events: &Arc<dyn EventSink>, folders: Vec<PathBuf>, stop: &AtomicBool) {
    for folder in folders {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let repo = repo_at(&folder);
        if stop.load(Ordering::Relaxed) || events.deliver(Event::Repo { folder, repo }).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests;
