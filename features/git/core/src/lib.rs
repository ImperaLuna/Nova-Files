#![forbid(unsafe_code)]

mod git;
mod git_dir;
mod marks;
mod repo_config;
mod status;
mod watch;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use nova_files_api::{ClientId, EventSink};
use nova_files_git_api::{Event, Mark, Marks};

pub use marks::marks_at;
pub use status::repo_at;
use watch::{GitWatch, Stop, Woke};

const THREAD: &str = "nova-files-git";

#[derive(Default)]
pub struct Reader {
    repos: Runs,
    marks: Runs,
}

impl Reader {
    pub fn read(&self, client: ClientId, events: &Arc<dyn EventSink>, folders: Vec<PathBuf>) {
        self.repos.forget(client);
        if folders.is_empty() {
            return;
        }
        let events = Arc::clone(events);
        self.repos
            .start(client, move |stop| follow_repos(&events, &folders, stop));
    }

    pub fn mark(&self, client: ClientId, events: &Arc<dyn EventSink>, folder: Option<PathBuf>) {
        self.marks.forget(client);
        let Some(folder) = folder else {
            return;
        };
        let events = Arc::clone(events);
        self.marks
            .start(client, move |stop| follow_marks(&events, &folder, stop));
    }

    pub fn forget(&self, client: ClientId) {
        self.repos.forget(client);
        self.marks.forget(client);
    }
}

#[derive(Default)]
struct Runs {
    running: Mutex<HashMap<ClientId, Arc<Stop>>>,
}

impl Runs {
    fn start(&self, client: ClientId, work: impl FnOnce(&Stop) + Send + 'static) {
        let stop = match Stop::new() {
            Ok(stop) => Arc::new(stop),
            Err(error) => {
                log::error!("no way to stop reading git state: {error}");
                return;
            }
        };
        self.lock().insert(client, Arc::clone(&stop));
        let started = thread::Builder::new()
            .name(THREAD.to_owned())
            .spawn(move || work(&stop));
        if let Err(error) = started {
            log::error!("no thread to read git state: {error}");
        }
    }

    fn forget(&self, client: ClientId) {
        let running = self.lock().remove(&client);
        if let Some(running) = running {
            running.stop();
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<ClientId, Arc<Stop>>> {
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn watching(add: impl FnOnce(&mut GitWatch)) -> Option<GitWatch> {
    match GitWatch::new() {
        Ok(mut watch) => {
            add(&mut watch);
            Some(watch)
        }
        Err(error) => {
            log::warn!("git state won't follow changes: {error}");
            None
        }
    }
}

fn follow_repos(events: &Arc<dyn EventSink>, folders: &[PathBuf], stop: &Stop) {
    let watch = watching(|watch| {
        for (index, folder) in folders.iter().enumerate() {
            watch.add(index, folder);
        }
    });
    if !read_each(events, folders.iter(), stop) {
        return;
    }
    let Some(mut watch) = watch else { return };
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

fn follow_marks(events: &Arc<dyn EventSink>, folder: &Path, stop: &Stop) {
    let top = marks::repo_top(folder).map(|(top, _)| top);
    let mut watch = watching(|watch| {
        watch.add_folder(0, folder);
        if let Some(top) = &top {
            watch.add(0, top);
        }
    });
    let marks = marks_at(folder);
    if stop.stopped() {
        return;
    }
    if let (Some(watch), Some(marks)) = (&mut watch, &marks) {
        watch.add_subfolders(0, folder, |name| {
            marks.entries.get(name) == Some(&Mark::Ignored)
        });
    }
    if !send_marks(events, folder, marks) || top.is_none() {
        return;
    }
    let Some(mut watch) = watch else { return };
    loop {
        match watch.next(stop) {
            Ok(None) => return,
            Ok(Some(_)) => {}
            Err(error) => {
                log::warn!("stopped following git marks: {error}");
                return;
            }
        }
        let marks = marks_at(folder);
        if stop.stopped() {
            return;
        }
        if !send_marks(events, folder, marks) {
            return;
        }
    }
}

fn send_marks(events: &Arc<dyn EventSink>, folder: &Path, marks: Option<Marks>) -> bool {
    let event = Event::Marks {
        folder: folder.to_path_buf(),
        marks,
    };
    events.deliver(event).is_ok()
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
