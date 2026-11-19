use std::collections::HashSet;
use std::ops::Range;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;

use nova_files_api::{
    ClientGone, Details, EntryId, Event, EventSink, FileIcon, Listing, ListingId, MAX_ROWS,
};

use crate::details;
use nova_files_disk::ports::Icons;

const BATCH: usize = 256;
const WORKER_THREAD: &str = "nova-files-details";

pub struct Queue {
    shared: Arc<Shared>,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

#[derive(Default)]
struct State {
    listing: Option<Arc<Listing>>,
    known: Option<Arc<[Details]>>,
    wanted: Range<usize>,
    read: HashSet<usize>,
    closed: bool,
}

impl State {
    fn next_run(&self) -> Option<Run> {
        let listed = self.listing.clone()?;
        let unread = |index: &usize| !self.read.contains(index);
        let start = self.wanted.clone().find(unread)?;
        let end = (start..self.wanted.end)
            .take(BATCH)
            .take_while(unread)
            .last()
            .map_or(start, |last| last + 1);
        Some(Run {
            listed,
            known: self.known.clone(),
            entries: start..end,
        })
    }
}

struct Run {
    listed: Arc<Listing>,
    known: Option<Arc<[Details]>>,
    entries: Range<usize>,
}

impl Queue {
    pub fn start(events: Arc<dyn EventSink>, icons: Arc<dyn Icons>) -> Option<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
        });
        let worker = Arc::clone(&shared);
        thread::Builder::new()
            .name(WORKER_THREAD.to_owned())
            .spawn(move || work(&worker, events.as_ref(), icons.as_ref()))
            .ok()?;
        Some(Self { shared })
    }

    pub fn show(
        &self,
        listing: &Listing,
        known: Option<Arc<[Details]>>,
        current: impl FnOnce() -> bool,
    ) {
        self.shared.update(|state| {
            if !current() {
                return;
            }
            state.listing = Some(Arc::new(listing.clone()));
            state.known = known;
            state.wanted = 0..0;
            state.read.clear();
        });
    }

    pub fn want(&self, id: ListingId, first: EntryId, count: u32) {
        self.shared.update(|state| {
            if let Some(listing) = &state.listing
                && listing.id == id
            {
                let first = usize::try_from(first.0).unwrap_or(usize::MAX);
                let count = usize::try_from(count.min(MAX_ROWS)).unwrap_or_default();
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

    fn still_wanted(&self, run: &Run) -> bool {
        let state = self.lock();
        !state.closed
            && state
                .listing
                .as_ref()
                .is_some_and(|listing| listing.id == run.listed.id)
            && state.wanted.start < run.entries.end
            && run.entries.start < state.wanted.end
    }

    fn next_run(&self) -> Option<Run> {
        let mut state = self.lock();
        loop {
            if state.closed {
                return None;
            }
            if let Some(run) = state.next_run() {
                return Some(run);
            }
            state = self
                .wake
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    #[expect(
        clippy::significant_drop_tightening,
        reason = "the lock is held through the send on purpose"
    )]
    fn send(
        &self,
        run: &Run,
        read: Vec<Details>,
        icons: Vec<FileIcon>,
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
            icons: icons.into(),
        })
    }
}

fn work(shared: &Shared, events: &dyn EventSink, icons: &dyn Icons) {
    while let Some(run) = shared.next_run() {
        let read = if let Some(known) = &run.known {
            Some(known.get(run.entries.clone()).unwrap_or_default().to_vec())
        } else {
            let entries = details::listed(&run.listed, run.entries.clone());
            details::read_batch(entries, || !shared.still_wanted(&run))
        };
        let Some(read) = read else {
            continue;
        };
        if !shared.still_wanted(&run) {
            continue;
        }
        let found = details::entries(&run.listed, run.entries.clone())
            .iter()
            .map(|entry| icons.of(&entry.name, entry.kind))
            .collect();
        if shared.send(&run, read, found, events).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::sync::mpsc;

    use nova_files_api::{Entry, EntryKind};

    use super::*;

    fn state(count: u32, wanted: Range<usize>, read: &[usize]) -> State {
        let entries: Arc<[Entry]> = (0..count)
            .map(|index| Entry {
                id: EntryId(index),
                name: OsString::from(index.to_string()),
                kind: EntryKind::File,
            })
            .collect();
        State {
            listing: Some(Arc::new(Listing {
                id: ListingId(0),
                path: PathBuf::new(),
                entries,
                folders: None,
            })),
            wanted,
            known: None,
            read: read.iter().copied().collect(),
            closed: false,
        }
    }

    fn shared(state: State) -> Shared {
        Shared {
            state: Mutex::new(state),
            wake: Condvar::new(),
        }
    }

    fn next(state: &State) -> Option<Range<usize>> {
        state.next_run().map(|run| run.entries)
    }

    fn run(id: u64, entries: Range<usize>) -> Run {
        Run {
            listed: Arc::new(Listing {
                id: ListingId(id),
                path: PathBuf::new(),
                entries: Arc::new([]),
                folders: None,
            }),
            known: None,
            entries,
        }
    }

    #[test]
    fn a_run_holds_at_most_one_batch() {
        assert_eq!(next(&state(600, 0..512, &[])), Some(0..BATCH));
    }

    #[test]
    fn a_run_starts_at_the_first_unread_row_and_stops_at_a_read_one() {
        assert_eq!(next(&state(10, 0..10, &[0, 1, 4])), Some(2..4));
    }

    #[test]
    fn a_run_of_one_row_holds_that_row() {
        assert_eq!(next(&state(10, 0..10, &[0, 2])), Some(1..2));
    }

    #[test]
    fn a_run_stops_at_the_end_of_what_is_wanted() {
        assert_eq!(next(&state(10, 3..6, &[])), Some(3..6));
    }

    #[test]
    fn nothing_to_read_once_every_wanted_row_is_read() {
        assert_eq!(next(&state(10, 3..5, &[3, 4])), None);
    }

    #[test]
    fn a_run_is_wanted_while_some_of_it_is_asked_for() {
        let queue = shared(state(100, 10..20, &[]));
        assert!(queue.still_wanted(&run(0, 15..30)));
        assert!(queue.still_wanted(&run(0, 0..11)));
        assert!(queue.still_wanted(&run(0, 19..20)));
    }

    #[test]
    fn a_run_next_to_what_is_asked_for_is_not_wanted() {
        let queue = shared(state(100, 10..20, &[]));
        assert!(!queue.still_wanted(&run(0, 0..10)));
        assert!(!queue.still_wanted(&run(0, 20..30)));
    }

    #[test]
    fn a_run_of_another_listing_or_a_closed_queue_is_not_wanted() {
        let queue = shared(state(100, 0..20, &[]));
        assert!(!queue.still_wanted(&run(1, 0..10)));
        queue.update(|state| state.closed = true);
        assert!(!queue.still_wanted(&run(0, 0..10)));
    }

    #[test]
    fn details_of_a_replaced_listing_are_not_sent() {
        let queue = shared(state(10, 0..10, &[]));
        let (sink, events) = mpsc::channel::<Event>();

        queue
            .send(&run(1, 0..1), vec![Details::default()], Vec::new(), &sink)
            .ok();

        assert!(events.try_recv().is_err());
        assert!(queue.lock().read.is_empty());
    }

    #[test]
    fn dropping_the_queue_stops_the_worker() {
        let shared = Arc::new(shared(state(10, 0..10, &[])));
        let queue = Queue {
            shared: Arc::clone(&shared),
        };

        drop(queue);

        assert!(shared.next_run().is_none());
    }
}
