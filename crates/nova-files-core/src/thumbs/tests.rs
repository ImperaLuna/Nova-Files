//! The queue's own rules: which entries are still wanted, what a batch
//! holds, and that dropping it stops its worker.

use std::sync::mpsc;
use std::time::Duration;

use nova_files_api::ClientGone;

use super::*;

const WAIT: Duration = Duration::from_secs(5);
const LISTING: ListingId = ListingId(1);

fn entry(index: u32, name: &str, kind: EntryKind) -> Entry {
    Entry {
        id: EntryId(index),
        name: name.into(),
        kind,
    }
}

fn shared(entries: Vec<Entry>, wanted: Range<usize>, done: &[usize]) -> Shared {
    Shared {
        state: Mutex::new(State {
            shown: Some(Arc::new(Shown {
                id: LISTING,
                folder: PathBuf::from("/folder"),
                entries: entries.into(),
            })),
            wanted,
            done: done.iter().copied().collect(),
            closed: false,
        }),
        wake: Condvar::new(),
    }
}

#[test]
fn only_wanted_entries_of_the_shown_open_listing_are_still_wanted() {
    let files = || {
        vec![
            entry(0, "a", EntryKind::File),
            entry(1, "b", EntryKind::File),
        ]
    };
    let queue = shared(files(), 0..1, &[]);
    assert!(queue.still_wanted(LISTING, 0));
    assert!(!queue.still_wanted(LISTING, 1));
    assert!(!queue.still_wanted(ListingId(2), 0));

    queue.update(|state| state.closed = true);
    assert!(!queue.still_wanted(LISTING, 0));
}

#[test]
fn a_batch_holds_only_wanted_files_not_done_yet() {
    let entries = vec![
        entry(0, "folder", EntryKind::Folder),
        entry(1, "new.png", EntryKind::File),
        entry(2, "done.png", EntryKind::File),
    ];
    let queue = Arc::new(shared(entries, 0..3, &[2]));

    // On a thread, so a queue that waits instead fails the test.
    let (sender, batches) = mpsc::channel();
    let worker = Arc::clone(&queue);
    thread::spawn(move || sender.send(worker.next_batch()));
    let (shown, batch) = batches.recv_timeout(WAIT).unwrap().unwrap();

    assert_eq!((shown.id, batch), (LISTING, vec![1]));
    let state = queue.state.lock().unwrap();
    assert_eq!(state.done, HashSet::from([1, 2]));
}

/// A client that never listens; its channel closes once it is dropped.
struct Gone(#[expect(dead_code, reason = "only dropped")] mpsc::Sender<()>);

impl EventSink for Gone {
    fn deliver(&self, _event: Event) -> Result<(), ClientGone> {
        Err(ClientGone)
    }
}

#[test]
fn dropping_the_queue_stops_its_worker() {
    let (gone, dropped) = mpsc::channel();
    let setup = Thumbnails {
        folder: PathBuf::from("/nonexistent/thumbnails"),
        helper: PathBuf::from("/nonexistent/helper"),
    };
    let queue = Queue::start(setup, Arc::new(Gone(gone))).unwrap();

    drop(queue);

    // The worker drops the client once it returns.
    assert_eq!(
        dropped.recv_timeout(WAIT),
        Err(mpsc::RecvTimeoutError::Disconnected)
    );
}
