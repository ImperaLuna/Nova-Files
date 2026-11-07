//! The queue's own rules, and its worker against a stand-in helper.

use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt as _;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use nova_files_api::ClientGone;

use super::*;

const WAIT: Duration = Duration::from_secs(5);
const LISTING: ListingId = ListingId(1);
/// Every test file's mtime, in seconds.
const MTIME: u64 = 1_000_000;
/// Stands in for the helper: notes each run's files in `runs`, then saves
/// `made.png` as the thumbnail of each.
const HELPER_SCRIPT: &str = r#"#!/bin/sh
dir=$1
shift
echo "$@" >> "$dir/runs"
while [ $# -ge 4 ]; do
    cp "$dir/made.png" "$dir/normal/$4"
    shift 4
done
"#;

/// A folder under the system temp folder, removed when dropped.
struct TempFolder(PathBuf);

impl TempFolder {
    fn new(test: &str) -> Self {
        let path = env::temp_dir().join(format!("nova-files-queue-{test}-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

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

/// A thumbnail or marker as the cache reads it: a `Thumb::MTime` text and
/// an end chunk.
fn cached_png(mtime: u64) -> Vec<u8> {
    let text = [b"Thumb::MTime\0".as_slice(), mtime.to_string().as_bytes()].concat();
    cache::png(&[(b"tEXt", &text), (b"IEND", b"")])
}

fn make_file(folder: &Path, name: &str) -> String {
    let path = folder.join(name);
    let file = File::create(&path).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_secs(MTIME))
        .unwrap();
    cache::name(&uri::file_uri(&path))
}

fn next_thumbnail(events: &Receiver<Event>) -> (EntryId, Vec<u8>) {
    match events.recv_timeout(WAIT).unwrap() {
        Event::Thumbnail { id, entry, png } if id == LISTING => (entry, png.to_vec()),
        other => panic!("expected a thumbnail, got {other:?}"),
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

#[test]
fn cached_files_answer_marked_ones_are_skipped_and_the_rest_go_to_the_helper() {
    let temp = TempFolder::new("worker");
    let folder = temp.0.join("media");
    let thumbnails = temp.0.join("thumbnails");
    for dir in [
        &folder,
        &cache::normal(&thumbnails),
        &cache::failed(&thumbnails),
    ] {
        fs::create_dir_all(dir).unwrap();
    }
    let names: Vec<String> = ["a.png", "b.png", "broken.png", "new.png"]
        .iter()
        .map(|name| make_file(&folder, name))
        .collect();
    let cached = cached_png(MTIME);
    fs::write(cache::normal(&thumbnails).join(&names[0]), &cached).unwrap();
    fs::write(cache::normal(&thumbnails).join(&names[1]), &cached).unwrap();
    fs::write(cache::failed(&thumbnails).join(&names[2]), &cached).unwrap();
    let mut made = cached_png(MTIME);
    made.extend_from_slice(b"made by the helper");
    fs::write(thumbnails.join("made.png"), &made).unwrap();
    let helper = temp.0.join("helper.sh");
    fs::write(&helper, HELPER_SCRIPT).unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();

    let (sender, events) = mpsc::channel();
    let setup = Thumbnails {
        folder: thumbnails.clone(),
        helper,
    };
    let queue = Queue::start(setup, Arc::new(sender)).unwrap();
    let entries: Vec<Entry> = ["a.png", "b.png", "broken.png", "new.png"]
        .iter()
        .zip(0..)
        .map(|(name, index)| entry(index, name, EntryKind::File))
        .collect();
    queue.show(LISTING, folder.clone(), entries.into(), || true);
    queue.want(LISTING, EntryId(0), 4);

    assert_eq!(next_thumbnail(&events), (EntryId(0), cached.clone()));
    assert_eq!(next_thumbnail(&events), (EntryId(1), cached));
    assert_eq!(next_thumbnail(&events), (EntryId(3), made));
    let runs = fs::read_to_string(thumbnails.join("runs")).unwrap();
    let new = folder.join("new.png");
    let expected = format!(
        "{} {} {MTIME} {}\n",
        new.display(),
        uri::file_uri(&new),
        names[3]
    );
    assert_eq!(runs, expected);
}

#[test]
fn the_cache_is_found_from_the_environment() {
    let program = Some(PathBuf::from("/app/bin/nova-files"));
    let helper = PathBuf::from("/app/bin/nova-files-thumbnailer");
    let from_var = Thumbnails::from(
        Some("/var/cache".into()),
        Some("/home/me".into()),
        program.clone(),
    );
    assert_eq!(
        from_var,
        Some(Thumbnails {
            folder: PathBuf::from("/var/cache/thumbnails"),
            helper: helper.clone(),
        })
    );
    // A relative cache folder is ignored, as the spec says.
    let from_home = Thumbnails::from(Some("cache".into()), Some("/home/me".into()), program);
    assert_eq!(
        from_home,
        Some(Thumbnails {
            folder: PathBuf::from("/home/me/.cache/thumbnails"),
            helper,
        })
    );
    assert_eq!(
        Thumbnails::from(None, None, Some("/app/nova-files".into())),
        None
    );
    assert_eq!(
        Thumbnails::from(Some("/var/cache".into()), None, None),
        None
    );
}
