//! Reading details only for the entries a client shows.
//!
//! Each client has a [`Queue`] with a worker thread of its own. A listing
//! goes out with names alone; the client then names the entries it shows
//! or is about to show (see `Command::Rows`), and the worker reads the
//! details of those, a run of up to [`BATCH`] neighbours at a time, in
//! entry order. An entry's details are read once per listing, so scrolling
//! back costs nothing. Entries never shown are never read: a 100,000-entry
//! folder costs a few screens of reads, not 100,000. A new listing replaces
//! the old one and its work; the worker asks before each read whether its
//! listing is still the one shown.

use std::collections::HashSet;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;

use nova_files_api::{ClientGone, Details, Entry, EntryId, Event, EventSink, ListingId};

use crate::details;

/// Entries per [`Event::DetailsRead`]: a few screens of rows, so the
/// rows asked for arrive in one or two events, each one redraw.
const BATCH: usize = 256;
/// The most entries one request may name: several screens of rows.
const MAX_WANTED: u32 = 512;
const WORKER_THREAD: &str = "nova-files-details";

/// One client's details work. Dropping it stops the worker.
pub struct Queue {
    shared: Arc<Shared>,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

#[derive(Default)]
struct State {
    listing: Option<Arc<Listed>>,
    wanted: Range<usize>,
    /// Entries of the listing whose details were sent.
    read: HashSet<usize>,
    closed: bool,
}

/// The listing on the client's screen.
struct Listed {
    id: ListingId,
    folder: PathBuf,
    entries: Arc<[Entry]>,
}

/// The next entries to read: `entries` of listing `listed`.
struct Run {
    listed: Arc<Listed>,
    entries: Range<usize>,
}

impl Queue {
    /// Starts the worker; `None` when the OS refuses a thread.
    pub fn start(events: Arc<dyn EventSink>) -> Option<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
        });
        let worker = Arc::clone(&shared);
        thread::Builder::new()
            .name(WORKER_THREAD.to_owned())
            .spawn(move || work(&worker, events.as_ref()))
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
            state.listing = Some(Arc::new(Listed {
                id,
                folder,
                entries,
            }));
            state.wanted = 0..0;
            state.read.clear();
        });
    }

    /// The client wants the details of `count` entries of listing `id` from
    /// `first`, in place of what it wanted before.
    pub fn want(&self, id: ListingId, first: EntryId, count: u32) {
        self.shared.update(|state| {
            if let Some(listing) = &state.listing
                && listing.id == id
            {
                let first = usize::try_from(first.0).unwrap_or(usize::MAX);
                let count = usize::try_from(count.min(MAX_WANTED)).unwrap_or_default();
                let end = first.saturating_add(count).min(listing.entries.len());
                state.wanted = first.min(end)..end;
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
        let mut state = self.lock();
        change(&mut state);
        drop(state);
        self.wake.notify_all();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether listing `id` is still the one shown.
    fn current(&self, id: ListingId) -> bool {
        let state = self.lock();
        !state.closed
            && state
                .listing
                .as_ref()
                .is_some_and(|listing| listing.id == id)
    }

    /// Waits for the next run: the first wanted entry not read yet and the
    /// unread ones right after it, up to [`BATCH`]. `None` once closed.
    fn next_run(&self) -> Option<Run> {
        let mut state = self.lock();
        loop {
            if state.closed {
                return None;
            }
            if let Some(listed) = state.listing.clone() {
                let unread = |index: &usize| !state.read.contains(index);
                if let Some(start) = state.wanted.clone().find(unread) {
                    let end = (start..state.wanted.end)
                        .take(BATCH)
                        .take_while(unread)
                        .last()
                        .map_or(start, |last| last + 1);
                    return Some(Run {
                        listed,
                        entries: start..end,
                    });
                }
            }
            state = self
                .wake
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Sends the details of `run`, read as `read`, unless its listing was
    /// replaced meanwhile. Marking them read and sending happen under one
    /// lock, so a new listing can't slip in between.
    ///
    /// # Errors
    /// [`ClientGone`] when the client stopped listening.
    #[expect(
        clippy::significant_drop_tightening,
        reason = "the lock is held through the send on purpose"
    )]
    fn send(
        &self,
        run: &Run,
        read: Vec<Details>,
        events: &dyn EventSink,
    ) -> Result<(), ClientGone> {
        let mut state = self.lock();
        let current = state
            .listing
            .as_ref()
            .is_some_and(|listing| listing.id == run.listed.id);
        let Ok(first) = u32::try_from(run.entries.start) else {
            return Ok(());
        };
        if state.closed || !current {
            return Ok(());
        }
        state.read.extend(run.entries.clone());
        events.deliver(Event::DetailsRead {
            id: run.listed.id,
            first: EntryId(first),
            details: read.into(),
        })
    }
}

fn work(shared: &Shared, events: &dyn EventSink) {
    while let Some(run) = shared.next_run() {
        let listed = &run.listed;
        // `want` keeps the wanted entries inside the listing.
        let entries = listed.entries.get(run.entries.clone()).unwrap_or_default();
        let Some(read) =
            details::read_batch(&listed.folder, entries, || !shared.current(listed.id))
        else {
            continue;
        };
        if shared.send(&run, read, events).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use nova_files_api::EntryKind;

    use super::*;

    /// A queue state showing `count` files, wanting `wanted`, with `read`
    /// already sent.
    fn shared(count: u32, wanted: Range<usize>, read: &[usize]) -> Shared {
        let entries: Arc<[Entry]> = (0..count)
            .map(|index| Entry {
                id: EntryId(index),
                name: OsString::from(index.to_string()),
                kind: EntryKind::File,
            })
            .collect();
        let state = State {
            listing: Some(Arc::new(Listed {
                id: ListingId(0),
                folder: PathBuf::new(),
                entries,
            })),
            wanted,
            read: read.iter().copied().collect(),
            closed: false,
        };
        Shared {
            state: Mutex::new(state),
            wake: Condvar::new(),
        }
    }

    fn next(shared: &Shared) -> Range<usize> {
        shared.next_run().map(|run| run.entries).unwrap_or_default()
    }

    #[test]
    fn a_run_holds_at_most_one_batch() {
        assert_eq!(next(&shared(600, 0..512, &[])), 0..BATCH);
    }

    #[test]
    fn a_run_starts_at_the_first_unread_row_and_stops_at_a_read_one() {
        assert_eq!(next(&shared(10, 0..10, &[0, 1, 4])), 2..4);
    }

    #[test]
    fn a_run_stops_at_the_end_of_what_is_wanted() {
        assert_eq!(next(&shared(10, 3..6, &[])), 3..6);
    }
}
