//! Measuring how much space files and folders take, for the client that
//! asks.
//!
//! [`Measurer::measure`] starts a scan on a thread of its own, which walks
//! the paths on a few threads (see `walk`) into a tally (see `tally`). As
//! folders are read, the client hears the totals so far at most every
//! [`EVERY`], then once more when the scan ends. A scan changes nothing on
//! the disk, so it is not a job: only its client hears of it.
//!
//! A client has at most one scan, like one search: asking again stops the
//! one before, and so do [`Command::StopMeasure`] and the client leaving;
//! such a scan says nothing more. [`Measurer::stop_under`] stops the scans
//! under a folder (a drive about to be unmounted) and waits until they
//! have let go of it; their clients hear [`ScanState::CutShort`].
//!
//! Each scan's state is set under the lock of the scans map. The walk asks
//! it before each read from the disk, and every answer goes out under that
//! same lock after asking it, so a stopped scan never says more than it
//! should.

mod tally;
mod walk;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use nova_files_api::{ClientGone, ClientId, Command, Event, EventSink, MeasureId, ScanState, Size};

use tally::Tally;
use walk::{Root, walk};

/// The least time between two reports of one scan.
const EVERY: Duration = Duration::from_millis(100);

/// The longest [`Measurer::stop_under`] waits for scans to let go: a scan
/// stuck in a read from a drive that stopped answering may never.
pub const LET_GO_WAIT: Duration = Duration::from_secs(5);

const THREAD_NAME: &str = "nova-files-measure";

/// What a scan that couldn't start reports: nothing could be read.
const UNMEASURED: Size = Size {
    bytes: 0,
    on_disk: 0,
    files: 0,
    folders: 0,
    unreadable: 1,
};

/// A scan that runs.
const RUNNING: u8 = 0;
/// A scan its client stopped or replaced: it says nothing more.
const STOPPED: u8 = 1;
/// A scan the core cut short: its client hears so, once.
const CUT: u8 = 2;

/// One scan, as the map and its threads share it.
struct Shared {
    id: MeasureId,
    roots: Vec<PathBuf>,
    /// [`RUNNING`], [`STOPPED`] or [`CUT`].
    state: AtomicU8,
    /// Set once the scan's walk is over and its folders are closed.
    ended: Mutex<bool>,
    ended_signal: Condvar,
}

impl Shared {
    fn running(&self) -> bool {
        self.state.load(Ordering::SeqCst) == RUNNING
    }

    /// Moves a running scan to `state`; a scan already stopped stays so.
    fn halt(&self, state: u8) {
        self.state
            .compare_exchange(RUNNING, state, Ordering::SeqCst, Ordering::SeqCst)
            .ok();
    }

    fn end(&self) {
        *self.ended.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.ended_signal.notify_all();
    }

    /// Waits until the scan ended, up to `deadline`; whether it did.
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

/// Each client's scan.
type Scans = HashMap<ClientId, Arc<Shared>>;

/// The scans running, one per client at most.
#[derive(Default)]
pub struct Measurer {
    scans: Arc<Mutex<Scans>>,
}

impl Measurer {
    /// Answers `client`'s [`Command::Measure`] or [`Command::StopMeasure`];
    /// any other command is not a scan's and is left alone.
    pub fn answer(&self, client: ClientId, events: &Arc<dyn EventSink>, command: Command) {
        match command {
            Command::Measure { id, paths } => self.measure(client, events, id, paths),
            Command::StopMeasure(id) => self.stop(client, id),
            _ => {}
        }
    }

    /// Measures everything under `paths` together for `client`, as scan
    /// `id`, stopping the client's scan before.
    pub fn measure(
        &self,
        client: ClientId,
        events: &Arc<dyn EventSink>,
        id: MeasureId,
        paths: Vec<PathBuf>,
    ) {
        let shared = Arc::new(Shared {
            id,
            roots: paths.clone(),
            state: AtomicU8::new(RUNNING),
            ended: Mutex::new(false),
            ended_signal: Condvar::new(),
        });
        let old = lock(&self.scans).insert(client, Arc::clone(&shared));
        if let Some(old) = old {
            old.halt(STOPPED);
        }
        let scan = Scan {
            scans: Arc::clone(&self.scans),
            client,
            shared,
            events: Arc::clone(events),
        };
        let fallback = scan.clone();
        let spawned = thread::Builder::new()
            .name(THREAD_NAME.to_owned())
            .spawn(move || {
                let roots: Vec<Root> = outermost(paths)
                    .into_iter()
                    .map(|path| Root { path, slot: 0 })
                    .collect();
                let tally = Tally::new(1);
                let report = |state| Event::SizeMeasured {
                    id,
                    size: tally.total(),
                    state,
                };
                scan.run(&roots, &tally, report);
                scan.finish();
            });
        if spawned.is_err() {
            let (size, state) = (UNMEASURED, ScanState::Done);
            fallback.tell(Event::SizeMeasured { id, size, state }).ok();
            fallback.finish();
        }
    }

    /// Stops `client`'s scan `id`, if it still runs. It says nothing more.
    pub fn stop(&self, client: ClientId, id: MeasureId) {
        let mut scans = lock(&self.scans);
        let current = scans.get(&client).is_some_and(|scan| scan.id == id);
        if current && let Some(scan) = scans.remove(&client) {
            scan.halt(STOPPED);
        }
    }

    /// Stops the scan of `client`, which left.
    pub fn forget(&self, client: ClientId) {
        let scan = lock(&self.scans).remove(&client);
        if let Some(scan) = scan {
            scan.halt(STOPPED);
        }
    }

    /// Cuts short every scan of a path under `folder` (a drive about to be
    /// unmounted), and waits until each has closed what it opened there,
    /// up to [`LET_GO_WAIT`]. Whether they all did. Each scan's client
    /// hears [`ScanState::CutShort`].
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "drives (FIL-35) call it before unmounting")
    )]
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

/// One running scan, as its threads see it.
#[derive(Clone)]
struct Scan {
    scans: Arc<Mutex<Scans>>,
    client: ClientId,
    shared: Arc<Shared>,
    events: Arc<dyn EventSink>,
}

impl Scan {
    /// Hands `event` to the client while the scan runs; under the scans'
    /// lock, so a stop set before can't be missed.
    fn tell(&self, event: Event) -> Result<(), ClientGone> {
        let scans = lock(&self.scans);
        if !self.shared.running() {
            return Ok(());
        }
        let told = self.events.deliver(event);
        drop(scans);
        told
    }

    /// Walks `roots` into `tally`, telling the client what `report` makes
    /// of it as it goes and once at the end. A client that stopped
    /// listening stops the scan.
    fn run(&self, roots: &[Root], tally: &Tally, report: impl Fn(ScanState) -> Event + Sync) {
        // Also held while a report goes out, so two threads never send
        // theirs out of order.
        let told: Mutex<Option<Instant>> = Mutex::new(None);
        let progress = || {
            let mut told = told.lock().unwrap_or_else(PoisonError::into_inner);
            let due = told.is_none_or(|told| told.elapsed() >= EVERY);
            if due {
                *told = Some(Instant::now());
                if self.tell(report(ScanState::Counting)).is_err() {
                    self.shared.halt(STOPPED);
                }
            }
            drop(told);
        };
        walk(roots, tally, &|| !self.shared.running(), &progress);
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

    /// Forgets this scan, unless a newer one of the client took its place,
    /// and tells anyone waiting that it let go of its folders.
    fn finish(&self) {
        let mut scans = lock(&self.scans);
        if scans
            .get(&self.client)
            .is_some_and(|current| Arc::ptr_eq(current, &self.shared))
        {
            scans.remove(&self.client);
        }
        drop(scans);
        self.shared.end();
    }
}

/// `paths` less any named twice or inside another, which a walk of that
/// other counts already: it never follows a link, so that is the only way
/// one path reaches another's files.
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
