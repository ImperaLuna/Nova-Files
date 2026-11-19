mod space;
mod tally;
mod walk;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use nova_files_api::{ClientGone, ClientId, Unreadable};
use nova_files_messages::EventSink;
use nova_files_properties_api::{Event, MeasureId, ScanState, Size};

use tally::Tally;
use walk::{Root, walk};

const EVERY: Duration = Duration::from_millis(100);

pub const LET_GO_WAIT: Duration = Duration::from_secs(5);

const THREAD_NAME: &str = "nova-files-measure";

const UNMEASURED: Size = Size {
    bytes: 0,
    on_disk: 0,
    files: 0,
    folders: 0,
    unreadable: 1,
};

const RUNNING: u8 = 0;
const STOPPED: u8 = 1;
const CUT: u8 = 2;

struct Shared {
    id: MeasureId,
    roots: Vec<PathBuf>,
    state: AtomicU8,
    ended: Mutex<bool>,
    ended_signal: Condvar,
}

impl Shared {
    const fn new(id: MeasureId, roots: Vec<PathBuf>) -> Self {
        Self {
            id,
            roots,
            state: AtomicU8::new(RUNNING),
            ended: Mutex::new(false),
            ended_signal: Condvar::new(),
        }
    }

    fn running(&self) -> bool {
        self.state.load(Ordering::SeqCst) == RUNNING
    }

    fn halt(&self, state: u8) {
        self.state
            .compare_exchange(RUNNING, state, Ordering::SeqCst, Ordering::SeqCst)
            .ok();
    }

    fn end(&self) {
        *self.ended.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.ended_signal.notify_all();
    }

    fn wait_ended(&self, deadline: Instant) -> bool {
        let ended = self.ended.lock().unwrap_or_else(PoisonError::into_inner);
        let left = deadline.saturating_duration_since(Instant::now());
        let (ended, _) = self
            .ended_signal
            .wait_timeout_while(ended, left, |ended| !*ended)
            .unwrap_or_else(PoisonError::into_inner);
        *ended
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Purpose {
    Total,
    Space,
}

type Scans = HashMap<(ClientId, Purpose), Arc<Shared>>;

#[derive(Default)]
pub struct Measurer {
    scans: Arc<Mutex<Scans>>,
}

impl Measurer {
    pub fn measure(
        &self,
        client: ClientId,
        events: &Arc<dyn EventSink>,
        id: MeasureId,
        paths: Vec<PathBuf>,
    ) {
        let key = (client, Purpose::Total);
        self.start(key, events, Shared::new(id, paths.clone()), move |scan| {
            let roots: Vec<Root> = outermost(paths)
                .into_iter()
                .map(|path| Root {
                    path,
                    slot: 0,
                    listed_in: None,
                })
                .collect();
            let tally = Tally::new(1);
            let report = |state| Event::SizeMeasured {
                id,
                size: tally.total(),
                state,
            };
            scan.run(&roots, &tally, report);
        });
    }

    pub fn space(
        &self,
        client: ClientId,
        events: &Arc<dyn EventSink>,
        id: MeasureId,
        folder: PathBuf,
    ) {
        let key = (client, Purpose::Space);
        self.start(
            key,
            events,
            Shared::new(id, vec![folder.clone()]),
            move |scan| {
                space::run(scan, id, &folder);
            },
        );
    }

    fn start(
        &self,
        key: (ClientId, Purpose),
        events: &Arc<dyn EventSink>,
        shared: Shared,
        work: impl FnOnce(&Scan) + Send + 'static,
    ) {
        let shared = Arc::new(shared);
        let old = lock(&self.scans).insert(key, Arc::clone(&shared));
        if let Some(old) = old {
            old.halt(STOPPED);
        }
        let scan = Scan {
            scans: Arc::clone(&self.scans),
            key,
            shared,
            events: Arc::clone(events),
        };
        let fallback = scan.clone();
        let spawned = thread::Builder::new()
            .name(THREAD_NAME.to_owned())
            .spawn(move || {
                work(&scan);
                scan.finish();
            });
        if spawned.is_err() {
            fallback.tell(|| fallback.unstarted()).ok();
            fallback.finish();
        }
    }

    pub fn stop(&self, client: ClientId, id: MeasureId) {
        let mut scans = lock(&self.scans);
        for purpose in [Purpose::Total, Purpose::Space] {
            let key = (client, purpose);
            let current = scans.get(&key).is_some_and(|scan| scan.id == id);
            if current && let Some(scan) = scans.remove(&key) {
                scan.halt(STOPPED);
            }
        }
    }

    pub fn forget(&self, client: ClientId) {
        lock(&self.scans).retain(|&(owner, _), scan| {
            if owner == client {
                scan.halt(STOPPED);
            }
            owner != client
        });
    }

    pub fn stop_under(&self, folder: &Path) -> bool {
        let under: Vec<Arc<Shared>> = lock(&self.scans)
            .values()
            .filter(|scan| scan.roots.iter().any(|root| root.starts_with(folder)))
            .map(|scan| {
                scan.halt(CUT);
                Arc::clone(scan)
            })
            .collect();
        let deadline = Instant::now() + LET_GO_WAIT;
        under.iter().all(|scan| scan.wait_ended(deadline))
    }
}

#[derive(Clone)]
struct Scan {
    scans: Arc<Mutex<Scans>>,
    key: (ClientId, Purpose),
    shared: Arc<Shared>,
    events: Arc<dyn EventSink>,
}

impl Scan {
    fn unstarted(&self) -> Event {
        let (id, state) = (self.shared.id, ScanState::Done);
        match self.key.1 {
            Purpose::Total => Event::SizeMeasured {
                id,
                size: UNMEASURED,
                state,
            },
            Purpose::Space => Event::SpaceUnreadable {
                id,
                reason: Unreadable::Failed,
            },
        }
    }

    fn tell(&self, event: impl FnOnce() -> Event) -> Result<(), ClientGone> {
        let scans = lock(&self.scans);
        if !self.shared.running() {
            return Ok(());
        }
        let told = self.events.deliver(event());
        drop(scans);
        told
    }

    fn last(&self, report: impl FnOnce(ScanState) -> Event) {
        let scans = lock(&self.scans);
        let state = match self.shared.state.load(Ordering::SeqCst) {
            RUNNING => Some(ScanState::Done),
            CUT => Some(ScanState::CutShort),
            _ => None,
        };
        if let Some(state) = state {
            self.events.deliver(report(state)).ok();
        }
        drop(scans);
    }

    fn run(&self, roots: &[Root], tally: &Tally, report: impl Fn(ScanState) -> Event + Sync) {
        let told: Mutex<Option<Instant>> = Mutex::new(None);
        let progress = || {
            let mut told = told.lock().unwrap_or_else(PoisonError::into_inner);
            let due = told.is_none_or(|told| told.elapsed() >= EVERY);
            if due {
                *told = Some(Instant::now());
                if self.tell(|| report(ScanState::Counting)).is_err() {
                    self.shared.halt(STOPPED);
                }
            }
            drop(told);
        };
        walk(roots, tally, &|| !self.shared.running(), &progress);
        self.last(report);
    }

    fn finish(&self) {
        let mut scans = lock(&self.scans);
        if scans
            .get(&self.key)
            .is_some_and(|current| Arc::ptr_eq(current, &self.shared))
        {
            scans.remove(&self.key);
        }
        drop(scans);
        self.shared.end();
    }
}

fn outermost(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.sort();
    paths.dedup();
    paths
        .iter()
        .filter(|path| {
            !paths
                .iter()
                .any(|outer| outer != *path && path.starts_with(outer))
        })
        .cloned()
        .collect()
}

fn lock(scans: &Mutex<Scans>) -> MutexGuard<'_, Scans> {
    scans.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests;
