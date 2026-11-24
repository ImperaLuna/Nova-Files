#![forbid(unsafe_code)]

mod git;
mod git_dir;
mod repo_config;
mod status;
mod watch;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use nova_files_api::{ClientId, EventSink};
use nova_files_git_api::Event;

pub use status::repo_at;
use watch::{GitWatch, Stop, Woke};

const THREAD: &str = "nova-files-git";

#[derive(Default)]
pub struct Reader {
    reading: Mutex<HashMap<ClientId, Arc<Stop>>>,
}

impl Reader {
    pub fn read(&self, client: ClientId, events: &Arc<dyn EventSink>, folders: Vec<PathBuf>) {
        self.forget(client);
        if folders.is_empty() {
            return;
        }
        let stop = match Stop::new() {
            Ok(stop) => Arc::new(stop),
            Err(error) => {
                log::error!("no way to stop reading git state: {error}");
                return;
            }
        };
        self.lock().insert(client, Arc::clone(&stop));
        let events = Arc::clone(events);
        let started = thread::Builder::new()
            .name(THREAD.to_owned())
            .spawn(move || follow(&events, &folders, &stop));
        if let Err(error) = started {
            log::error!("no thread to read git state: {error}");
        }
    }

    pub fn forget(&self, client: ClientId) {
        let reading = self.lock().remove(&client);
        if let Some(reading) = reading {
            reading.stop();
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<ClientId, Arc<Stop>>> {
        self.reading.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn follow(events: &Arc<dyn EventSink>, folders: &[PathBuf], stop: &Stop) {
    let watch = GitWatch::new().map(|mut watch| {
        for (index, folder) in folders.iter().enumerate() {
            watch.add(index, folder);
        }
        watch
    });
    if !read_each(events, folders.iter(), stop) {
        return;
    }
    let mut watch = match watch {
        Ok(watch) => watch,
        Err(error) => {
            log::warn!("git state won't follow changes: {error}");
            return;
        }
    };
    loop {
        let again: Vec<&PathBuf> = match watch.next(stop) {
            Ok(None) => return,
            Ok(Some(Woke::All)) => folders.iter().collect(),
            Ok(Some(Woke::Folders(changed))) => changed
                .iter()
                .filter_map(|&index| folders.get(index))
                .collect(),
            Err(error) => {
                log::warn!("stopped following git state: {error}");
                return;
            }
        };
        if !read_each(events, again.into_iter(), stop) {
            return;
        }
    }
}

fn read_each<'a>(
    events: &Arc<dyn EventSink>,
    folders: impl Iterator<Item = &'a PathBuf>,
    stop: &Stop,
) -> bool {
    for folder in folders {
        if stop.stopped() {
            return false;
        }
        let repo = repo_at(folder);
        if stop.stopped() {
            return false;
        }
        let event = Event::Repo {
            folder: folder.clone(),
            repo,
        };
        if events.deliver(event).is_err() {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests;
