#![forbid(unsafe_code)]

mod cache;
mod md5;
mod peek;
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
    EntryId, EntryKind, EventSink, FileTypes, Listing, ListingId, MAX_ROWS, ThumbnailSize, uri,
};
use nova_files_disk::home;
use nova_files_entries_core::RowWatch;
use nova_files_thumbnailer_launch as launch;
use nova_files_thumbnails_api::{Event, Peeked};

const THUMBNAILS_FOLDER: &str = "thumbnails";
const HELPER_NAME: &str = "nova-files-thumbnailer";
const BATCH: usize = 8;
const WORKERS: usize = 6;
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

impl State {
    fn wants(&self, shown: &Arc<Listing>, index: usize, size: ThumbnailSize) -> bool {
        !self.closed
            && self
                .shown
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, shown))
            && self.size == size
            && self.wanted.contains(&index)
    }
}

impl Queue {
    pub fn start(
        setup: Thumbnails,
        events: Arc<dyn EventSink>,
        types: Arc<dyn FileTypes>,
    ) -> Option<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
        });
        let setup = Arc::new(Setup {
            paths: setup,
            types,
        });
        for events in std::iter::repeat_n(events, WORKERS) {
            let worker = Arc::clone(&shared);
            let setup = Arc::clone(&setup);
            let started = thread::Builder::new()
                .name(WORKER_THREAD.to_owned())
                .spawn(move || work(&worker, &setup, events.as_ref()));
            if started.is_err() {
                shared.update(|state| state.closed = true);
                return None;
            }
        }
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

    fn still_wanted(&self, shown: &Arc<Listing>, index: usize, size: ThumbnailSize) -> bool {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.wants(shown, index, size)
    }

    #[expect(
        clippy::significant_drop_tightening,
        reason = "the lock is held through the send on purpose"
    )]
    fn deliver_if_wanted(
        &self,
        (shown, index, size): (&Arc<Listing>, usize, ThumbnailSize),
        event: impl FnOnce() -> Option<Event>,
        events: &dyn EventSink,
    ) -> bool {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if !state.wants(shown, index, size) {
            return true;
        }
        event().is_none_or(|event| events.deliver(event).is_ok())
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
                        !state.done.contains(index) && shown.entries.get(*index).is_some()
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

struct Setup {
    paths: Thumbnails,
    types: Arc<dyn FileTypes>,
}

fn work(shared: &Shared, setup: &Setup, events: &dyn EventSink) {
    while let Some(Batch {
        shown,
        size,
        entries: batch,
    }) = shared.next_batch()
    {
        let (folders, files): (Vec<usize>, Vec<usize>) = batch.into_iter().partition(|index| {
            shown
                .entries
                .get(*index)
                .is_some_and(|entry| entry.kind == EntryKind::Folder)
        });
        let wanted = |index: usize| shared.still_wanted(&shown, index, size);
        let mut client_here = thumbnails((&shown, size), &files, setup, &wanted, |index, png| {
            let event = || {
                Some(Event::Thumbnail {
                    id: shown.id,
                    entry: EntryId(u32::try_from(index).ok()?),
                    size,
                    png: png.into(),
                })
            };
            shared.deliver_if_wanted((&shown, index, size), event, events)
        });
        for index in folders {
            if !client_here {
                return;
            }
            if !wanted(index) {
                continue;
            }
            let Some(peeked) = peek_into(&shown, index, size, setup, || !wanted(index)) else {
                continue;
            };
            let event = || {
                Some(Event::Peek {
                    id: shown.id,
                    entry: EntryId(u32::try_from(index).ok()?),
                    size,
                    peeked,
                })
            };
            client_here = shared.deliver_if_wanted((&shown, index, size), event, events);
        }
        if !client_here {
            return;
        }
    }
}

fn thumbnails(
    (shown, size): (&Listing, ThumbnailSize),
    indexes: &[usize],
    setup: &Setup,
    wanted: &dyn Fn(usize) -> bool,
    answer: impl FnMut(usize, Vec<u8>) -> bool,
) -> bool {
    let jobs = indexes
        .iter()
        .filter(|index| wanted(**index))
        .filter_map(|index| {
            let entry = shown.entries.get(*index)?;
            Some((*index, job_at(shown.path_of(entry))?))
        })
        .collect();
    make((&setup.paths, size), jobs, wanted, answer)
}

fn peek_into(
    shown: &Listing,
    index: usize,
    size: ThumbnailSize,
    setup: &Setup,
    stop: impl Fn() -> bool,
) -> Option<Vec<Peeked>> {
    let folder = shown.path_of(shown.entries.get(index)?);
    let picks = peek::look(&folder, setup.types.as_ref(), &stop)?;
    let mut peeked: Vec<Peeked> = picks
        .iter()
        .map(|pick| Peeked {
            name: pick.entry.name.clone(),
            kind: pick.entry.kind,
            png: None,
        })
        .collect();
    let jobs = picks
        .iter()
        .enumerate()
        .filter(|(_, pick)| pick.picture && !stop())
        .filter_map(|(at, pick)| Some((at, job_at(folder.join(&pick.entry.name))?)))
        .collect();
    let going = |_: usize| !stop();
    make((&setup.paths, size), jobs, &going, |at, png| {
        if let Some(shown) = peeked.get_mut(at) {
            shown.png = Some(png.into());
        }
        true
    });
    (!stop()).then_some(peeked)
}

fn make(
    (paths, size): (&Thumbnails, ThumbnailSize),
    jobs: Vec<(usize, launch::Job)>,
    wanted: &dyn Fn(usize) -> bool,
    mut answer: impl FnMut(usize, Vec<u8>) -> bool,
) -> bool {
    let sized = cache::sized(&paths.folder, size);
    let failed = cache::failed(&paths.folder);
    let mut misses = Vec::new();
    for (at, job) in jobs {
        if !wanted(at) {
            continue;
        }
        if let Some(png) = cache::read_if_current(&sized, &job.name, job.mtime) {
            if !answer(at, png) {
                return false;
            }
        } else if wanted(at) && cache::read_if_current(&failed, &job.name, job.mtime).is_none() {
            misses.push((at, job));
        }
    }
    misses.retain(|(at, _)| wanted(*at));
    if misses.is_empty() {
        return true;
    }
    let jobs: Vec<launch::Job> = misses.iter().map(|(_, job)| job.clone()).collect();
    let mut client_here = true;
    launch::run(&paths.helper, &paths.folder, size, &jobs, |made| {
        let Some((at, job)) = misses.get(made) else {
            return client_here;
        };
        if wanted(*at)
            && let Some(png) = cache::read_if_current(&sized, &job.name, job.mtime)
        {
            client_here = answer(*at, png);
        }
        client_here
    });
    client_here
}

fn job_at(source: PathBuf) -> Option<launch::Job> {
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
