#![forbid(unsafe_code)]

mod cache;
mod md5;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::env;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;
use std::time::UNIX_EPOCH;

use nova_files_api::{
    EntryId, EntryKind, EventSink, Listing, ListingId, MAX_ROWS, ThumbnailSize, uri,
};
use nova_files_disk::home;
use nova_files_entries_core::RowWatch;
use nova_files_thumbnailer_launch as launch;
use nova_files_thumbnails_api::Event;

const THUMBNAILS_FOLDER: &str = "thumbnails";
const HELPER_NAME: &str = "nova-files-thumbnailer";
const BATCH: usize = 8;
const WORKER_THREAD: &str = "nova-files-thumbnails";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumbnails {
    pub folder: PathBuf,
    pub helper: PathBuf,
}

impl Thumbnails {
    pub fn from_env() -> Option<Self> {
        let cache = home::cache_home()?;
        let helper = env::current_exe().ok()?.parent()?.join(HELPER_NAME);
        Some(Self {
            folder: cache.join(THUMBNAILS_FOLDER),
            helper,
        })
    }
}

pub struct Queue {
    shared: Arc<Shared>,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

#[derive(Default)]
struct State {
    shown: Option<Arc<Listing>>,
    wanted: Range<usize>,
    size: ThumbnailSize,
    done: HashSet<usize>,
    closed: bool,
}

impl Queue {
    pub fn start(setup: Thumbnails, events: Arc<dyn EventSink>) -> Option<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
        });
        let worker = Arc::clone(&shared);
        thread::Builder::new()
            .name(WORKER_THREAD.to_owned())
            .spawn(move || work(&worker, &setup, events.as_ref()))
            .ok()?;
        Some(Self { shared })
    }

    pub fn show(&self, listing: &Listing, current: impl FnOnce() -> bool) {
        self.shared.update(|state| {
            if !current() {
                return;
            }
            state.shown = Some(Arc::new(listing.clone()));
            state.wanted = 0..0;
            state.done.clear();
        });
    }

    pub fn want(&self, id: ListingId, first: EntryId, count: u32, size: Option<ThumbnailSize>) {
        self.shared.update(|state| {
            if state.shown.as_ref().is_some_and(|shown| shown.id == id) {
                let first = usize::try_from(first.0).unwrap_or(usize::MAX);
                let count = usize::try_from(count.min(MAX_ROWS)).unwrap_or_default();
                state.wanted = first..first.saturating_add(count);
                if let Some(size) = size.filter(|size| *size != state.size) {
                    state.size = size;
                    state.done.clear();
                }
                let wanted = state.wanted.clone();
                state.done.retain(|index| wanted.contains(index));
            }
        });
    }
}

impl RowWatch for Queue {
    fn show(&self, listing: &Listing, current: &dyn Fn() -> bool) {
        Self::show(self, listing, current);
    }

    fn want(&self, id: ListingId, first: EntryId, count: u32, size: Option<ThumbnailSize>) {
        Self::want(self, id, first, count, size);
    }
}

impl Drop for Queue {
    fn drop(&mut self) {
        self.shared.update(|state| state.closed = true);
    }
}

impl Shared {
    fn update(&self, change: impl FnOnce(&mut State)) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        change(&mut state);
        drop(state);
        self.wake.notify_all();
    }

    fn still_wanted(&self, id: ListingId, index: usize, size: ThumbnailSize) -> bool {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        !state.closed
            && state.shown.as_ref().is_some_and(|shown| shown.id == id)
            && state.size == size
            && state.wanted.contains(&index)
    }

    fn next_batch(&self) -> Option<Batch> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if state.closed {
                return None;
            }
            if let Some(shown) = state.shown.clone() {
                let batch: Vec<usize> = state
                    .wanted
                    .clone()
                    .filter(|index| {
                        !state.done.contains(index)
                            && shown
                                .entries
                                .get(*index)
                                .is_some_and(|entry| entry.kind == EntryKind::File)
                    })
                    .take(BATCH)
                    .collect();
                if !batch.is_empty() {
                    state.done.extend(&batch);
                    return Some(Batch {
                        shown,
                        size: state.size,
                        entries: batch,
                    });
                }
            }
            state = self
                .wake
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

struct Batch {
    shown: Arc<Listing>,
    size: ThumbnailSize,
    entries: Vec<usize>,
}

fn work(shared: &Shared, setup: &Thumbnails, events: &dyn EventSink) {
    while let Some(Batch {
        shown,
        size,
        entries: batch,
    }) = shared.next_batch()
    {
        let answer = |index: usize, png: Vec<u8>| {
            if !shared.still_wanted(shown.id, index, size) {
                return true;
            }
            let Ok(entry) = u32::try_from(index) else {
                return true;
            };
            events
                .deliver(Event::Thumbnail {
                    id: shown.id,
                    entry: EntryId(entry),
                    size,
                    png: png.into(),
                })
                .is_ok()
        };
        let wanted = |index: usize| shared.still_wanted(shown.id, index, size);
        let sized = cache::sized(&setup.folder, size);
        let failed = cache::failed(&setup.folder);
        let mut misses = Vec::new();
        for index in batch {
            if !wanted(index) {
                continue;
            }
            let Some(job) = job_for(&shown, index) else {
                continue;
            };
            if !wanted(index) {
                continue;
            }
            if let Some(png) = cache::read_if_current(&sized, &job.name, job.mtime) {
                if !answer(index, png) {
                    return;
                }
            } else if wanted(index)
                && cache::read_if_current(&failed, &job.name, job.mtime).is_none()
            {
                misses.push((index, job));
            }
        }
        misses.retain(|(index, _)| wanted(*index));
        if misses.is_empty() {
            continue;
        }
        let jobs: Vec<launch::Job> = misses.iter().map(|(_, job)| job.clone()).collect();
        launch::run(&setup.helper, &setup.folder, size, &jobs);
        for (index, job) in misses {
            if !wanted(index) {
                continue;
            }
            if let Some(png) = cache::read_if_current(&sized, &job.name, job.mtime)
                && !answer(index, png)
            {
                return;
            }
        }
    }
}

fn job_for(shown: &Listing, index: usize) -> Option<launch::Job> {
    let entry = shown.entries.get(index)?;
    let source = shown.path_of(entry);
    let mtime = fs::metadata(&source)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs();
    let uri = uri::file_uri(&absolute(&source));
    let name = cache::name(&uri);
    Some(launch::Job {
        source,
        uri,
        mtime,
        name,
    })
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_owned())
}
