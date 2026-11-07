//! Folder listing through the real core, against real temp folder trees:
//! the order, links, empty folders, names that aren't UTF-8, and folders
//! that can't be read.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt as _;
use std::os::unix::fs::{PermissionsExt as _, symlink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use nova_files_api::{
    Command, Connection, EntryId, EntryKind, Event, Listing, ListingId, Unreadable,
};

/// How long a test waits for the core before it counts as a failure.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);
/// No permissions at all.
const NO_ACCESS: u32 = 0o000;

/// A folder under the system temp folder, removed when dropped.
struct TempFolder(PathBuf);

impl TempFolder {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "nova-files-core-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

struct Client {
    _core: nova_files_core::RunningCore,
    connection: Connection,
    events: Receiver<Event>,
}

fn client() -> Client {
    let core = nova_files_core::spawn().unwrap();
    let (sink, events) = mpsc::channel();
    let connection = core.connector.connect(sink).unwrap();
    Client {
        _core: core,
        connection,
        events,
    }
}

impl Client {
    fn list(&self, id: u64, path: &Path) -> Event {
        self.connection
            .send(Command::ListFolder {
                id: ListingId(id),
                path: path.to_owned(),
            })
            .unwrap();
        self.events.recv_timeout(ANSWER_TIMEOUT).unwrap()
    }
}

fn listing(event: Event) -> Listing {
    match event {
        Event::FolderListed(listing) => listing,
        other => panic!("expected a listing, got {other:?}"),
    }
}

fn names(listing: &Listing) -> Vec<(String, EntryKind)> {
    listing
        .entries
        .iter()
        .map(|entry| (entry.name.to_string_lossy().into_owned(), entry.kind))
        .collect()
}

#[test]
fn lists_folders_first_then_files_each_a_to_z() {
    let folder = TempFolder::new();
    let root = folder.path();
    fs::create_dir(root.join("pictures")).unwrap();
    fs::create_dir(root.join("Archive")).unwrap();
    fs::write(root.join("notes.txt"), "").unwrap();
    fs::write(root.join("Budget.ods"), "").unwrap();
    symlink(root.join("pictures"), root.join("link to pictures")).unwrap();
    symlink(root.join("gone"), root.join("broken link")).unwrap();

    let listing = listing(client().list(7, root));

    assert_eq!(listing.id, ListingId(7));
    assert_eq!(listing.path, root);
    assert_eq!(
        names(&listing),
        [
            ("Archive".to_owned(), EntryKind::Folder),
            ("link to pictures".to_owned(), EntryKind::Folder),
            ("pictures".to_owned(), EntryKind::Folder),
            ("broken link".to_owned(), EntryKind::File),
            ("Budget.ods".to_owned(), EntryKind::File),
            ("notes.txt".to_owned(), EntryKind::File),
        ]
    );
    let ids: Vec<EntryId> = listing.entries.iter().map(|entry| entry.id).collect();
    assert_eq!(ids, (0..6).map(EntryId).collect::<Vec<_>>());
}

#[test]
fn hidden_files_are_listed_like_any_other() {
    let folder = TempFolder::new();
    fs::write(folder.path().join(".hidden"), "").unwrap();

    let listing = listing(client().list(1, folder.path()));

    assert_eq!(names(&listing), [(".hidden".to_owned(), EntryKind::File)]);
}

#[test]
fn empty_folder_lists_nothing() {
    let folder = TempFolder::new();

    let listing = listing(client().list(1, folder.path()));

    assert_eq!(listing.entries.len(), 0);
}

#[test]
fn name_that_is_not_utf8_comes_back_unchanged() {
    let folder = TempFolder::new();
    let name = OsString::from_vec(vec![b'b', 0xff, b'd']);
    fs::write(folder.path().join(&name), "").unwrap();

    let listing = listing(client().list(1, folder.path()));

    assert_eq!(listing.entries.len(), 1);
    assert_eq!(listing.entries[0].name, name);
}

#[test]
fn missing_folder_is_reported_missing() {
    let folder = TempFolder::new();
    let gone = folder.path().join("gone");

    let event = client().list(3, &gone);

    assert_eq!(
        event,
        Event::FolderUnreadable {
            id: ListingId(3),
            path: gone,
            reason: Unreadable::Missing,
        }
    );
}

#[test]
fn file_is_not_a_folder() {
    let folder = TempFolder::new();
    let file = folder.path().join("file.txt");
    fs::write(&file, "").unwrap();

    let event = client().list(1, &file);

    assert!(matches!(
        event,
        Event::FolderUnreadable {
            reason: Unreadable::NotAFolder,
            ..
        }
    ));
}

#[test]
fn folder_without_permission_is_reported() {
    let folder = TempFolder::new();
    let locked = folder.path().join("locked");
    fs::create_dir(&locked).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(NO_ACCESS)).unwrap();
    // Root reads anything, so there is nothing to test when run as root.
    if fs::read_dir(&locked).is_ok() {
        return;
    }

    let event = client().list(1, &locked);

    assert!(matches!(
        event,
        Event::FolderUnreadable {
            reason: Unreadable::PermissionDenied,
            ..
        }
    ));
}

#[test]
fn places_answers_with_existing_folders_only() {
    let client = client();
    client.connection.send(Command::Places).unwrap();

    let Event::Places(places) = client.events.recv_timeout(ANSWER_TIMEOUT).unwrap() else {
        panic!("expected places");
    };

    assert!(places.iter().all(|place| place.path.is_dir()));
}
