#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;

use nova_files_api::{EventSink, ListingId};
use nova_files_entries_core::ListingJob;
use nova_files_index::{Change, ChangeLog, Index, LogReader, MatchKind, Overlay, STAGES, Watcher};
use nova_files_search_api::{Event, Hit, MAX_FOUND, MAX_HITS, Matched, SearchFailed, SearchId};

pub use complete::complete;
pub use index::{IndexError, IndexPlace, Unusable};

mod complete;
mod found;
mod index;

const INDEX_THREAD: &str = "nova-files-index";
const SEARCH_THREAD: &str = "nova-files-search";
const FIND_THREAD: &str = "nova-files-find";

pub const REBUILD_AFTER: usize = 10_000;

struct Query<'a> {
    text: &'a str,
    limit: usize,
    within: Option<&'a Path>,
}

impl<'a> Query<'a> {
    const fn everywhere(text: &'a str, limit: usize) -> Self {
        Self {
            text,
            limit,
            within: None,
        }
    }
}

#[derive(Clone)]
pub struct Searcher {
    shared: Arc<Shared>,
}

struct Shared {
    place: Option<(IndexPlace, PathBuf)>,
    state: Mutex<State>,
    built: Condvar,
}

#[derive(Default)]
struct State {
    live: Option<Live>,
    building: bool,
    failed: bool,
}

struct Live {
    index: Arc<Index>,
    overlay: Overlay,
    log: LogReader,
    changes: usize,
    lost: bool,
    watching: Watching,
}

struct Early {
    read: usize,
    shown: HashSet<PathBuf>,
}

enum Watching {
    CatchingUp,
    Running(Watcher),
    Stopped,
}

impl Searcher {
    pub fn new(place: Option<IndexPlace>, root: Option<PathBuf>) -> Self {
        Self {
            shared: Arc::new(Shared {
                place: place.zip(root),
                state: Mutex::default(),
                built: Condvar::new(),
            }),
        }
    }

    pub fn search(
        &self,
        (events, current): (&Arc<dyn EventSink>, &Arc<AtomicU64>),
        id: SearchId,
        query: String,
        limit: u32,
    ) {
        current.store(id.0, Ordering::SeqCst);
        let current = Arc::clone(current);
        let sink = Arc::clone(events);
        let cancelled = move || current.load(Ordering::SeqCst) != id.0;
        if self.shared.place.is_none() {
            let reason = SearchFailed::NoIndex;
            events.deliver(Event::SearchFailed { id, reason }).ok();
            return;
        }
        let searcher = self.clone();
        let limit = usize::try_from(limit.min(MAX_HITS)).unwrap_or_default();
        let spawned = thread::Builder::new()
            .name(SEARCH_THREAD.to_owned())
            .spawn(move || {
                searcher.start_build(false);
                let wanted = || !cancelled();
                let answer = |event| wanted() && sink.deliver(event).is_ok();
                if !searcher.run(&Query::everywhere(&query, limit), wanted, |hits, done| {
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
            events.deliver(Event::SearchFailed { id, reason }).ok();
        }
    }

    pub fn find(
        &self,
        (client, job): (&Arc<dyn EventSink>, ListingJob),
        id: ListingId,
        query: String,
        within: Option<PathBuf>,
    ) {
        let Some((_, root)) = &self.shared.place else {
            let reason = SearchFailed::NoIndex;
            job.answer(Event::FindFailed { id, reason });
            return;
        };
        let searcher = self.clone();
        let folder = within.clone().unwrap_or_else(|| root.clone());
        let limit = usize::try_from(MAX_FOUND).unwrap_or_default();
        let spawned = thread::Builder::new()
            .name(FIND_THREAD.to_owned())
            .spawn(move || {
                searcher.start_build(false);
                let mut hits = Vec::new();
                let wanted = || !job.cancelled();
                let asked = Query {
                    text: &query,
                    limit,
                    within: within.as_deref(),
                };
                let ran = searcher.run(&asked, wanted, |stage, _| {
                    hits.extend(stage);
                    true
                });
                if !ran {
                    let reason = SearchFailed::IndexFailed;
                    job.answer(Event::FindFailed { id, reason });
                } else if let Some(listing) = found::listing(id, folder, hits, || job.cancelled()) {
                    job.send(listing);
                }
            });
        if spawned.is_err() {
            let reason = SearchFailed::IndexFailed;
            client.deliver(Event::FindFailed { id, reason }).ok();
        }
    }

    fn run(
        &self,
        query: &Query<'_>,
        wanted: impl Fn() -> bool,
        mut answer: impl FnMut(Vec<Hit>, bool) -> bool,
    ) -> bool {
        let shared = &self.shared;
        let state = shared.state.lock().unwrap_or_else(PoisonError::into_inner);
        let mut state = shared
            .built
            .wait_while(state, |state| state.live.is_none() && state.building)
            .unwrap_or_else(PoisonError::into_inner);
        let failed = state.failed;
        let Some(live) = &mut state.live else {
            return false;
        };
        if live.lost && failed {
            drop(state);
            self.start_build(true);
            return false;
        }
        if !wanted() {
            return true;
        }
        live.read_log();
        let outdated = live.lost || live.changes > REBUILD_AFTER;
        let mut search = live
            .index
            .search_now(query.text, query.limit, &live.overlay);
        if let Some(folder) = query.within {
            search = search.within(folder);
        }
        if search.is_done() {
            answer(Vec::new(), true);
            return true;
        }
        let catching_up = matches!(live.watching, Watching::CatchingUp);
        let mut early = Early {
            read: live.changes,
            shown: HashSet::new(),
        };
        for _ in 0..STAGES {
            if !wanted() {
                return true;
            }
            let Some(hits) = search.next() else {
                break;
            };
            let last = search.is_done() && !catching_up;
            if catching_up {
                early.shown.extend(hits.iter().map(|hit| hit.path.clone()));
            }
            if !answer(hits.into_iter().map(told).collect(), last) {
                return true;
            }
        }
        drop(state);
        if catching_up {
            let late = self.changed_since(query, &early);
            if wanted() {
                answer(late, true);
            }
        } else if outdated {
            self.start_build(true);
        }
        true
    }

    fn changed_since(&self, query: &Query<'_>, early: &Early) -> Vec<Hit> {
        let shared = &self.shared;
        let state = shared.state.lock().unwrap_or_else(PoisonError::into_inner);
        let mut state = shared
            .built
            .wait_while(state, |state| {
                state
                    .live
                    .as_ref()
                    .is_some_and(|live| matches!(live.watching, Watching::CatchingUp))
            })
            .unwrap_or_else(PoisonError::into_inner);
        let Some(live) = &mut state.live else {
            return Vec::new();
        };
        live.read_log();
        if live.changes == early.read {
            return Vec::new();
        }
        let mut search = live
            .index
            .search_now(query.text, query.limit, &live.overlay);
        if let Some(folder) = query.within {
            search = search.within(folder);
        }
        let late: Vec<Hit> = search
            .flatten()
            .filter(|hit| !early.shown.contains(&hit.path))
            .take(query.limit)
            .map(told)
            .collect();
        drop(state);
        late
    }

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
                let opened =
                    panic::catch_unwind(AssertUnwindSafe(|| Live::open(&place, &root, fresh)))
                        .ok()
                        .flatten();
                let watch = opened.map(|(live, log)| {
                    let index = Arc::clone(&live.index);
                    job.settle(Some(live));
                    (index, log)
                });
                let Some((index, log)) = watch else {
                    job.settle(None);
                    return;
                };
                let watcher =
                    panic::catch_unwind(AssertUnwindSafe(|| Watcher::start(&index, log, || {})))
                        .ok()
                        .and_then(Result::ok);
                job.watch(&index, watcher);
            });
        if spawned.is_err() {
            let mut state = shared.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.building = false;
            state.failed = true;
            drop(state);
            shared.built.notify_all();
        }
    }
}

impl Shared {
    fn settle(&self, live: Option<Live>) {
        {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.building = false;
            state.failed = live.is_none();
            if live.is_some() {
                state.live = live;
            }
        }
        self.built.notify_all();
    }

    fn watch(&self, index: &Arc<Index>, watcher: Option<Watcher>) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let started = watcher.is_some();
        let current = state
            .live
            .as_mut()
            .filter(|live| Arc::ptr_eq(&live.index, index))
            .map(|live| live.watching = watcher.map_or(Watching::Stopped, Watching::Running))
            .is_some();
        if current {
            state.failed = !started;
        }
        drop(state);
        self.built.notify_all();
    }
}

impl Live {
    fn open(place: &IndexPlace, root: &Path, fresh: bool) -> Option<(Self, ChangeLog)> {
        let opened = if fresh { None } else { place.open(root).ok() };
        let index = if let Some(index) = opened {
            index
        } else {
            place.build(root).ok()?;
            place.open(root).ok()?
        };
        let log = ChangeLog::create(&place.changes(), index.crawled()).ok()?;
        let reader = LogReader::open(&place.changes()).ok()?;
        let live = Self {
            index: Arc::new(index),
            overlay: Overlay::new(),
            log: reader,
            changes: 0,
            lost: false,
            watching: Watching::CatchingUp,
        };
        Some((live, log))
    }

    fn read_log(&mut self) {
        let stopped = match &self.watching {
            Watching::CatchingUp => false,
            Watching::Running(watcher) => !watcher.is_running(),
            Watching::Stopped => true,
        };
        if stopped {
            self.lost = true;
        }
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

fn told(hit: nova_files_index::Hit) -> Hit {
    Hit {
        path: hit.path,
        matched: matched(hit.kind),
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
