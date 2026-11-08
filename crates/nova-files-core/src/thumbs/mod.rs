//! Thumbnails for the entries a client shows, from the shared freedesktop
//! cache or, when it has none, from the sandboxed helper.
//!
//! Each client has a [`Queue`] with a worker thread of its own. The client
//! names the entries it shows or is about to show; each request replaces
//! the one before. The worker takes the first entries still wanted, in
//! order, in batches: a current thumbnail or failure marker in the cache
//! answers at once, the rest go to the helper as one batch. Before running
//! the helper and before each answer it checks that the entry is still
//! wanted, so rows scrolled away cost nothing more. A new listing replaces
//! the old one and its work.

mod cache;
mod helper;
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

use nova_files_api::{Entry, EntryId, EntryKind, Event, EventSink, ListingId, MAX_ROWS, uri};

/// The thumbnails folder under the cache folder.
const THUMBNAILS_FOLDER: &str = "thumbnails";
const CACHE_HOME_VAR: &str = "XDG_CACHE_HOME";
const CACHE_IN_HOME: &str = ".cache";
/// The helper ships next to the program.
const HELPER_NAME: &str = "nova-files-thumbnailer";
/// Files per helper run: enough to spread the start-up cost, few enough
/// that the first thumbnails arrive quickly.
const BATCH: usize = 8;
const WORKER_THREAD: &str = "nova-files-thumbnails";

/// Where thumbnails go and the helper that makes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumbnails {
    /// The freedesktop thumbnails folder, shared with other apps.
    pub folder: PathBuf,
    pub helper: PathBuf,
}

impl Thumbnails {
    /// `$XDG_CACHE_HOME/thumbnails` (or `~/.cache/thumbnails`) and the
    /// helper next to the running program. `None` without a cache folder or
    /// a program path.
    pub fn from_env() -> Option<Self> {
        let cache = env::var_os(CACHE_HOME_VAR)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| env::home_dir().map(|home| home.join(CACHE_IN_HOME)))?;
        let helper = env::current_exe().ok()?.parent()?.join(HELPER_NAME);
        Some(Self {
            folder: cache.join(THUMBNAILS_FOLDER),
            helper,
        })
    }
}

/// One client's thumbnail work. Dropping it stops the worker.
pub struct Queue {
    shared: Arc<Shared>,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

#[derive(Default)]
struct State {
    shown: Option<Arc<Shown>>,
    wanted: Range<usize>,
    /// Entries of the shown listing already answered or given up on.
    done: HashSet<usize>,
    closed: bool,
}

/// The listing on the client's screen.
struct Shown {
    id: ListingId,
    folder: PathBuf,
    entries: Arc<[Entry]>,
}

impl Queue {
    /// Starts the worker; `None` when the OS refuses a thread.
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

    /// The client now shows listing `id`; earlier work is dropped. Called
    /// before the client hears of the listing, so its first request finds
    /// it. `current` is asked under the queue's lock, so a listing that was
    /// replaced meanwhile can't overwrite the one after it.
    pub fn show(
        &self,
        id: ListingId,
        folder: PathBuf,
        entries: Arc<[Entry]>,
        current: impl FnOnce() -> bool,
    ) {
        self.shared.update(|state| {
            if !current() {
                return;
            }
            state.shown = Some(Arc::new(Shown {
                id,
                folder,
                entries,
            }));
            state.wanted = 0..0;
            state.done.clear();
        });
    }

    /// The client wants thumbnails for `count` entries of listing `id` from
    /// `first`, in place of what it wanted before.
    pub fn want(&self, id: ListingId, first: EntryId, count: u32) {
        self.shared.update(|state| {
            if state.shown.as_ref().is_some_and(|shown| shown.id == id) {
                let first = usize::try_from(first.0).unwrap_or(usize::MAX);
                let count = usize::try_from(count.min(MAX_ROWS)).unwrap_or_default();
                state.wanted = first..first.saturating_add(count);
                // Rows the client stopped showing may be asked for again,
                // and it drops their thumbnails, so they count as not done.
                let wanted = state.wanted.clone();
                state.done.retain(|index| wanted.contains(index));
            }
        });
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

    /// Whether entry `index` of listing `id` is still wanted.
    fn still_wanted(&self, id: ListingId, index: usize) -> bool {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        !state.closed
            && state.shown.as_ref().is_some_and(|shown| shown.id == id)
            && state.wanted.contains(&index)
    }

    /// Waits for the next batch: the shown listing and up to [`BATCH`]
    /// wanted files not done yet, now marked done. `None` once closed.
    fn next_batch(&self) -> Option<(Arc<Shown>, Vec<usize>)> {
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
                    return Some((shown, batch));
                }
            }
            state = self
                .wake
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

fn work(shared: &Shared, setup: &Thumbnails, events: &dyn EventSink) {
    while let Some((shown, batch)) = shared.next_batch() {
        let answer = |index: usize, png: Vec<u8>| {
            if !shared.still_wanted(shown.id, index) {
                return true;
            }
            let Ok(entry) = u32::try_from(index) else {
                return true;
            };
            events
                .deliver(Event::Thumbnail {
                    id: shown.id,
                    entry: EntryId(entry),
                    png: png.into(),
                })
                .is_ok()
        };
        let wanted = |index: usize| shared.still_wanted(shown.id, index);
        let normal = cache::normal(&setup.folder);
        let failed = cache::failed(&setup.folder);
        let mut misses = Vec::new();
        // Each read waits for its row to be still wanted, so rows the client
        // moved away from cost nothing more.
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
            if let Some(png) = cache::read_if_current(&normal, &job.name, job.mtime) {
                if !answer(index, png) {
                    return;
                }
            } else if wanted(index)
                && cache::read_if_current(&failed, &job.name, job.mtime).is_none()
            {
                misses.push((index, job));
            }
        }
        // The client may have moved on while the cache was read.
        misses.retain(|(index, _)| wanted(*index));
        if misses.is_empty() {
            continue;
        }
        let jobs: Vec<helper::Job> = misses.iter().map(|(_, job)| job.clone()).collect();
        helper::run(&setup.helper, &setup.folder, &jobs);
        // The cache is the answer, not the helper's word: a helper killed
        // part-way still saved the files it finished.
        for (index, job) in misses {
            if !wanted(index) {
                continue;
            }
            if let Some(png) = cache::read_if_current(&normal, &job.name, job.mtime)
                && !answer(index, png)
            {
                return;
            }
        }
    }
}

/// What the cache and the helper need to know about entry `index`; `None`
/// when it vanished or has no mtime.
fn job_for(shown: &Shown, index: usize) -> Option<helper::Job> {
    let entry = shown.entries.get(index)?;
    let source = shown.folder.join(&entry.name);
    let mtime = fs::metadata(&source)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs();
    let uri = uri::file_uri(&absolute(&source));
    let name = cache::name(&uri);
    Some(helper::Job {
        source,
        uri,
        mtime,
        name,
    })
}

/// URIs name absolute paths; a listing's folder already is one, but a
/// relative one is made absolute rather than trusted.
fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_owned())
}
