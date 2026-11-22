use std::sync::mpsc;
use std::time::Duration;

use nova_files_api::{ClientGone, Entry, Envelope};
use nova_files_test_folder::TestFolder;

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

fn shown(shared: &Shared) -> Arc<Listing> {
    shared.state.lock().unwrap().shown.clone().unwrap()
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
    let listed = shown(&queue);
    let same_again = Arc::new(Listing::clone(&listed));
    assert!(queue.still_wanted(&listed, 0, normal));
    assert!(!queue.still_wanted(&listed, 1, normal));
    assert!(!queue.still_wanted(&same_again, 0, normal));
    assert!(!queue.still_wanted(&listed, 0, ThumbnailSize::Large));

    queue.update(|state| state.closed = true);
    assert!(!queue.still_wanted(&listed, 0, normal));
}

#[test]
fn a_batch_holds_only_wanted_entries_not_done_yet() {
    let entries = vec![
        entry(0, "folder", EntryKind::Folder),
        entry(1, "new.png", EntryKind::File),
        entry(2, "done.png", EntryKind::File),
    ];
    let queue = Arc::new(shared(entries, 0..3, &[2]));

    let (sender, batches) = mpsc::channel();
    let worker = Arc::clone(&queue);
    thread::spawn(move || sender.send(worker.next_batch()));
    let batch = batches.recv_timeout(WAIT).unwrap().unwrap();

    assert_eq!(
        (batch.shown.id, batch.size, batch.entries),
        (LISTING, ThumbnailSize::Normal, vec![0, 1])
    );
    let done = queue.state.lock().unwrap().done.clone();
    assert_eq!(done, HashSet::from([0, 1, 2]));
}

struct Gone(#[expect(dead_code, reason = "only dropped")] mpsc::Sender<()>);

impl EventSink for Gone {
    fn deliver_envelope(&self, _envelope: Envelope) -> Result<(), ClientGone> {
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
    let queue = Queue::start(setup, Arc::new(Gone(gone)), Arc::new(ByEnding)).unwrap();

    drop(queue);

    assert_eq!(
        dropped.recv_timeout(WAIT),
        Err(mpsc::RecvTimeoutError::Disconnected)
    );
}

#[test]
fn the_environment_names_the_thumbnails_folder_and_helper() {
    let thumbnails = Thumbnails::from_env().unwrap();
    let cache = home::cache_home().unwrap();
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

    queue.want(LISTING, EntryId(0), 2, Some(ThumbnailSize::Normal));
    assert_eq!(queue.shared.state.lock().unwrap().done.len(), 2);

    queue.want(LISTING, EntryId(0), 2, Some(ThumbnailSize::Large));
    let state = queue.shared.state.lock().unwrap();
    let (size, done) = (state.size, state.done.len());
    drop(state);
    assert_eq!((size, done), (ThumbnailSize::Large, 0));
}

#[test]
fn hiding_thumbnails_keeps_the_size() {
    let files = vec![entry(0, "a.png", EntryKind::File)];
    let queue = Queue {
        shared: Arc::new(shared(files, 0..1, &[0])),
    };
    queue.want(LISTING, EntryId(0), 1, Some(ThumbnailSize::Large));

    queue.want(LISTING, EntryId(0), 0, None);

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
    let before = shown(&queue.shared);
    queue.show(&newer, || false);
    assert!(queue.shared.still_wanted(&before, 0, ThumbnailSize::Normal));

    queue.show(&newer, || true);
    let state = queue.shared.state.lock().unwrap();
    assert_eq!(
        state.shown.as_ref().map(|shown| shown.id),
        Some(ListingId(2))
    );
    assert_eq!((state.wanted.clone(), state.done.len()), (0..0, 0));
    drop(state);
}

#[test]
fn a_found_file_is_thumbnailed_from_its_own_folder() {
    let test_folder = TestFolder::new("thumb-job");
    let folder = test_folder.path().to_path_buf();
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

    let job = job_at(found.path_of(&found.entries[0]));
    let gone = job_at(found.path_of(&found.entries[1]));

    let job = job.unwrap();
    assert_eq!(job.source, folder.join("photo.png"));
    assert!(job.mtime > 0);
    assert!(gone.is_none());
}

#[derive(Debug)]
struct ByEnding;

impl FileTypes for ByEnding {
    fn of(&self, name: &std::ffi::OsStr) -> Option<&str> {
        match Path::new(name).extension()?.to_str()? {
            "png" => Some("image/png"),
            "mp4" => Some("video/mp4"),
            "txt" => Some("text/plain"),
            _ => None,
        }
    }

    fn is_a(&self, mime: &str, wanted: &str) -> bool {
        wanted
            .strip_suffix('*')
            .map_or(mime == wanted, |media| mime.starts_with(media))
    }
}

fn names(picks: &[peek::Pick]) -> Vec<&str> {
    picks
        .iter()
        .map(|pick| pick.entry.name.to_str().unwrap())
        .collect()
}

fn numbered(found: &[(&str, EntryKind)]) -> Vec<Entry> {
    found
        .iter()
        .zip(0..)
        .map(|((name, kind), index)| entry(index, name, *kind))
        .collect()
}

#[test]
fn a_peek_takes_pictures_then_files_then_folders() {
    let entries = numbered(&[
        ("album", EntryKind::Folder),
        ("b.txt", EntryKind::File),
        ("c.png", EntryKind::File),
        (".d.png", EntryKind::File),
        ("e.mp4", EntryKind::File),
        ("f", EntryKind::File),
    ]);

    let picks = peek::pick(entries, &ByEnding);

    assert_eq!(names(&picks), ["c.png", "e.mp4", "b.txt"]);
    assert_eq!(
        picks.iter().map(|pick| pick.picture).collect::<Vec<_>>(),
        [true, true, false]
    );
    let few = numbered(&[("b.txt", EntryKind::File), ("album", EntryKind::Folder)]);
    assert_eq!(names(&peek::pick(few, &ByEnding)), ["b.txt", "album"]);
}

#[test]
fn a_peek_shows_hidden_entries_only_when_nothing_else_is_there() {
    let mixed = numbered(&[(".a", EntryKind::File), ("b.txt", EntryKind::File)]);
    assert_eq!(names(&peek::pick(mixed, &ByEnding)), ["b.txt"]);

    let hidden = numbered(&[(".a", EntryKind::File), (".b.png", EntryKind::File)]);
    assert_eq!(names(&peek::pick(hidden, &ByEnding)), [".b.png", ".a"]);

    assert_eq!(peek::pick(Vec::new(), &ByEnding), []);
}

struct Heard(mpsc::Sender<Event>);

impl EventSink for Heard {
    fn deliver_envelope(&self, envelope: Envelope) -> Result<(), ClientGone> {
        if let Ok(event) = envelope.open::<Event>() {
            self.0.send(event).map_err(|_| ClientGone)?;
        }
        Ok(())
    }
}

#[test]
fn folders_on_screen_get_a_peek_at_what_they_hold() {
    let test_folder = TestFolder::new("thumb-peek");
    let trip = test_folder.join("trip");
    fs::create_dir(&trip).unwrap();
    fs::write(trip.join("notes.txt"), b"hello").unwrap();
    fs::write(trip.join("photo.png"), b"not really a png").unwrap();
    fs::create_dir(test_folder.join("empty")).unwrap();
    let setup = Thumbnails {
        folder: test_folder.join("thumbnails"),
        helper: test_folder.join("no-helper"),
    };
    let (sender, heard) = mpsc::channel();
    let queue = Queue::start(setup, Arc::new(Heard(sender)), Arc::new(ByEnding)).unwrap();
    let listing = Listing {
        id: LISTING,
        path: test_folder.path().to_path_buf(),
        entries: numbered(&[("empty", EntryKind::Folder), ("trip", EntryKind::Folder)]).into(),
        folders: None,
    };

    queue.show(&listing, || true);
    queue.want(LISTING, EntryId(0), 2, Some(ThumbnailSize::Normal));

    let mut peeks: Vec<_> = (0..2)
        .map(|_| match heard.recv_timeout(WAIT).unwrap() {
            Event::Peek { entry, peeked, .. } => (entry, peeked),
            other @ Event::Thumbnail { .. } => {
                panic!("expected a peek, got {other:?}")
            }
        })
        .collect();
    peeks.sort_by_key(|(entry, _)| entry.0);
    let peeked = |name: &str| Peeked {
        name: name.into(),
        kind: EntryKind::File,
        png: None,
    };
    assert_eq!(
        peeks,
        [
            (EntryId(0), Vec::new()),
            (EntryId(1), vec![peeked("photo.png"), peeked("notes.txt")]),
        ]
    );
}
