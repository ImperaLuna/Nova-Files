//! Searching the home index by name, kept up to date by the watcher.
//!
//! Nothing is indexed until the first [`Command::Search`]: a core started
//! for a window that never searches never crawls. That search starts a
//! thread that opens the saved index (or crawls home when there is none, or
//! it is of another folder or version), starts a fresh change log and the
//! watcher, which catches up with what changed since the crawl. Searches
//! wait for it, then each reads the log's new changes into the overlay
//! before it runs, so results show the disk as it is now.
//!
//! After [`REBUILD_AFTER`] changes, or a [`Change::Rescan`] (changes were
//! lost), the next search starts a fresh crawl in the background and keeps
//! answering from the old index until the new one is in.
//!
//! [`Command::Search`]: nova_files_api::Command::Search

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;

use nova_files_api::{Event, Hit, MAX_HITS, Matched, SearchFailed, SearchId};
use nova_files_index::{Change, ChangeLog, Index, LogReader, MatchKind, Overlay, Watcher};

use crate::index::IndexPlace;
use crate::jobs::Client;

const INDEX_THREAD: &str = "nova-files-index";
const SEARCH_THREAD: &str = "nova-files-search";

/// Changes after which the index is crawled anew: the overlay only grows,
/// and putting hits through it costs more with each change.
pub const REBUILD_AFTER: usize = 10_000;

/// The home index, and what changed since its crawl.
#[derive(Clone)]
pub struct Searcher {
    shared: Arc<Shared>,
}

struct Shared {
    /// Where the index lives and the folder it covers; `None` without a
    /// home or cache folder.
    place: Option<(IndexPlace, PathBuf)>,
    state: Mutex<State>,
    /// Signalled when a build ends.
    built: Condvar,
}

#[derive(Default)]
struct State {
    live: Option<Live>,
    building: bool,
}

/// An open index with its watcher and what the log said so far.
struct Live {
    index: Index,
    overlay: Overlay,
    log: LogReader,
    changes: usize,
    /// Changes were lost, so the overlay no longer matches the disk.
    lost: bool,
    _watcher: Watcher,
}

impl Searcher {
    /// A searcher over `root`, its index kept in `place`.
    pub fn new(place: Option<IndexPlace>, root: Option<PathBuf>) -> Self {
        Self {
            shared: Arc::new(Shared {
                place: place.zip(root),
                state: Mutex::default(),
                built: Condvar::new(),
            }),
        }
    }

    /// Searches for `client`, replacing its search before.
    pub fn search(&self, client: &Client, id: SearchId, query: String, limit: u32) {
        client.current_search().store(id.0, Ordering::SeqCst);
        let current = Arc::clone(client.current_search());
        let events = Arc::clone(client.events());
        let cancelled = move || current.load(Ordering::SeqCst) != id.0;
        if self.shared.place.is_none() {
            let reason = SearchFailed::NoIndex;
            client.deliver(Event::SearchFailed { id, reason }).ok();
            return;
        }
        let searcher = self.clone();
        let limit = usize::try_from(limit.min(MAX_HITS)).unwrap_or_default();
        let spawned = thread::Builder::new()
            .name(SEARCH_THREAD.to_owned())
            .spawn(move || {
                // Here, not on the core's thread: a running search holds the
                // lock this takes.
                searcher.start_build(false);
                let wanted = || !cancelled();
                let answer = |event| wanted() && events.deliver(event).is_ok();
                if !searcher.run(&query, limit, wanted, |hits, done| {
                    answer(Event::Found { id, hits, done })
                }) {
                    answer(Event::SearchFailed {
                        id,
                        reason: SearchFailed::IndexFailed,
                    });
                }
            });
        if spawned.is_err() {
            let reason = SearchFailed::IndexFailed;
            client.deliver(Event::SearchFailed { id, reason }).ok();
        }
    }

    /// Waits for the index, then hands each stage's hits to `answer`
    /// until it returns `false`. Stops before reading the log or a stage
    /// once `wanted` says a newer search replaced this one. `false` when
    /// there is no index.
    fn run(
        &self,
        query: &str,
        limit: usize,
        wanted: impl Fn() -> bool,
        mut answer: impl FnMut(Vec<Hit>, bool) -> bool,
    ) -> bool {
        let shared = &self.shared;
        let state = shared.state.lock().unwrap_or_else(PoisonError::into_inner);
        let mut state = shared
            .built
            .wait_while(state, |state| state.live.is_none() && state.building)
            .unwrap_or_else(PoisonError::into_inner);
        let Some(live) = &mut state.live else {
            return false;
        };
        if !wanted() {
            return true;
        }
        live.read_log();
        let outdated = live.lost || live.changes > REBUILD_AFTER;
        let mut search = live.index.search_now(query, limit, &live.overlay);
        if search.is_done() {
            // Nothing to look for: no stages, one answer with no hits.
            answer(Vec::new(), true);
        }
        while wanted() {
            let Some(hits) = search.next() else {
                break;
            };
            let hits = hits.into_iter().map(|hit| Hit {
                path: hit.path,
                matched: matched(hit.kind),
            });
            if !answer(hits.collect(), search.is_done()) {
                break;
            }
        }
        drop(state);
        if outdated {
            self.start_build(true);
        }
        true
    }

    /// Opens or builds the index in the background unless one is open
    /// (`fresh` crawls anew even then) or a build is running.
    fn start_build(&self, fresh: bool) {
        let shared = &self.shared;
        let Some((place, root)) = &shared.place else {
            return;
        };
        let mut state = shared.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.building || (state.live.is_some() && !fresh) {
            return;
        }
        state.building = true;
        drop(state);
        let (job, place, root) = (Arc::clone(shared), place.clone(), root.to_owned());
        let spawned = thread::Builder::new()
            .name(INDEX_THREAD.to_owned())
            .spawn(move || {
                let live = Live::start(&place, &root, fresh);
                {
                    let mut state = job.state.lock().unwrap_or_else(PoisonError::into_inner);
                    state.building = false;
                    if live.is_some() {
                        state.live = live;
                    }
                }
                job.built.notify_all();
            });
        if spawned.is_err() {
            shared
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .building = false;
            shared.built.notify_all();
        }
    }
}

impl Live {
    /// Opens the index of `root` in `place`, or crawls when there is none
    /// or `fresh` asks, and starts following it.
    fn start(place: &IndexPlace, root: &Path, fresh: bool) -> Option<Self> {
        let opened = if fresh { None } else { place.open(root).ok() };
        let index = if let Some(index) = opened {
            index
        } else {
            place.build(root).ok()?;
            place.open(root).ok()?
        };
        let log = ChangeLog::create(&place.changes(), index.crawled()).ok()?;
        let reader = LogReader::open(&place.changes()).ok()?;
        // Searches read the log when they run; nothing to wake.
        let watcher = Watcher::start(&index, log, || {}).ok()?;
        Some(Self {
            index,
            overlay: Overlay::new(),
            log: reader,
            changes: 0,
            lost: false,
            _watcher: watcher,
        })
    }

    /// Takes in what the watcher logged since the last call.
    fn read_log(&mut self) {
        let Ok(changes) = self.log.read_new() else {
            self.lost = true;
            return;
        };
        self.take(&changes);
    }

    fn take(&mut self, changes: &[Change]) {
        self.changes += changes.len();
        for change in changes {
            self.lost |= matches!(change, Change::Rescan { .. });
            self.overlay.apply(change);
        }
    }
}

const fn matched(kind: MatchKind) -> Matched {
    match kind {
        MatchKind::Exact => Matched::Exact,
        MatchKind::WordStart => Matched::WordStart,
        MatchKind::Contains => Matched::Contains,
        MatchKind::InOrder => Matched::InOrder,
        MatchKind::OneTypo => Matched::OneTypo,
    }
}

#[cfg(test)]
mod tests;
