//! The queue's own rules: which entries are still wanted, what a batch
//! holds, and that dropping it stops its worker.

use std::sync::mpsc;
use std::time::Duration;

use nova_files_api::{ClientGone, Entry};

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
            shown: Some(Arc::new(Listing {
                id: LISTING,
                path: PathBuf::from("/folder"),
                entries: entries.into(),
                folders: None,
            })),
            wanted,
            size: ThumbnailSize::Normal,
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
    let normal = ThumbnailSize::Normal;
    let queue = shared(files(), 0..1, &[]);
    assert!(queue.still_wanted(LISTING, 0, normal));
    assert!(!queue.still_wanted(LISTING, 1, normal));
    assert!(!queue.still_wanted(ListingId(2), 0, normal));
    assert!(!queue.still_wanted(LISTING, 0, ThumbnailSize::Large));

    queue.update(|state| state.closed = true);
    assert!(!queue.still_wanted(LISTING, 0, normal));
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
    let batch = batches.recv_timeout(WAIT).unwrap().unwrap();

    assert_eq!(
        (batch.shown.id, batch.size, batch.entries),
        (LISTING, ThumbnailSize::Normal, vec![1])
    );
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

#[test]
fn the_environment_names_the_thumbnails_folder_and_helper() {
    // Tests run with a home folder and a program path.
    let thumbnails = Thumbnails::from_env().unwrap();
    let cache = crate::places::cache_home().unwrap();
    assert_eq!(thumbnails.folder, cache.join(THUMBNAILS_FOLDER));
    assert_eq!(thumbnails.helper.file_name().unwrap(), HELPER_NAME);
}

#[test]
fn a_new_size_sends_every_wanted_thumbnail_again() {
    let files = vec![
        entry(0, "a.png", EntryKind::File),
        entry(1, "b.png", EntryKind::File),
    ];
    let queue = Queue {
        shared: Arc::new(shared(files, 0..2, &[0, 1])),
    };

    queue.want(LISTING, EntryId(0), 2, ThumbnailSize::Normal);
    assert_eq!(queue.shared.state.lock().unwrap().done.len(), 2);

    queue.want(LISTING, EntryId(0), 2, ThumbnailSize::Large);
    let state = queue.shared.state.lock().unwrap();
    let (size, done) = (state.size, state.done.len());
    drop(state);
    assert_eq!((size, done), (ThumbnailSize::Large, 0));
}

#[test]
fn asking_for_no_rows_keeps_the_size() {
    let files = vec![entry(0, "a.png", EntryKind::File)];
    let queue = Queue {
        shared: Arc::new(shared(files, 0..1, &[0])),
    };
    queue.want(LISTING, EntryId(0), 1, ThumbnailSize::Large);

    queue.want(LISTING, EntryId(0), 0, ThumbnailSize::Normal);

    let state = queue.shared.state.lock().unwrap();
    let size = state.size;
    drop(state);
    assert_eq!(size, ThumbnailSize::Large);
}

#[test]
fn showing_a_listing_starts_over_unless_a_newer_one_came() {
    let queue = Queue {
        shared: Arc::new(shared(vec![entry(0, "a", EntryKind::File)], 0..1, &[0])),
    };
    let newer = Listing {
        id: ListingId(2),
        path: PathBuf::from("/other"),
        entries: Arc::new([]),
        folders: None,
    };
    queue.show(&newer, || false);
    assert!(queue.shared.still_wanted(LISTING, 0, ThumbnailSize::Normal));

    queue.show(&newer, || true);
    let state = queue.shared.state.lock().unwrap();
    assert_eq!(
        state.shown.as_ref().map(|shown| shown.id),
        Some(ListingId(2))
    );
    assert_eq!((state.wanted.clone(), state.done.len()), (0..0, 0));
}

#[test]
fn a_found_file_is_thumbnailed_from_its_own_folder() {
    let folder = env::temp_dir().join(format!("nova-files-thumb-job-{}", std::process::id()));
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("photo.png"), b"png").unwrap();
    let found = Listing {
        id: LISTING,
        path: PathBuf::from("/somewhere/else"),
        entries: Arc::new([
            entry(0, "photo.png", EntryKind::File),
            entry(1, "gone.png", EntryKind::File),
        ]),
        folders: Some(Arc::new([folder.clone(), folder.clone()])),
    };

    let job = job_for(&found, 0);
    let gone = job_for(&found, 1);

    fs::remove_dir_all(&folder).ok();
    let job = job.unwrap();
    assert_eq!(job.source, folder.join("photo.png"));
    assert!(job.mtime > 0);
    assert!(gone.is_none());
}
