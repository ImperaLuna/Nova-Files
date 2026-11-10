//! Folder listing through the real core, against real temp folder trees:
//! the order, links, empty folders, names that aren't UTF-8, folders that
//! can't be read, and the details of the rows a client shows.

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
    Command, Connection, Details, EntryId, EntryKind, Event, Listing, ListingId, PlaceKind,
    Unreadable,
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

/// A client of a core whose home is `home`, never the real one.
fn client_at(home: &Path) -> Client {
    let home = nova_files_core::Home {
        folder: home.to_owned(),
        config: home.join(".config"),
        state: home.join(".local/state"),
        data: home.join(".local/share"),
        drive_trashes: false,
    };
    let core =
        nova_files_core::spawn_with(nova_files_core::IDLE_EXIT, Some(home), None, None).unwrap();
    let (sink, events) = mpsc::channel();
    let connection = core.connector.connect(sink).unwrap();
    Client {
        _core: core,
        connection,
        events,
    }
}

/// A client of a core with an empty temp home.
fn client() -> Client {
    let home = TempFolder::new();
    let client = client_at(home.path());
    // The core reads the home folder only when asked for places.
    drop(home);
    client
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

    /// Shows rows `first..first + count` of listing `id` and returns the
    /// details that come back: the entry the first answer starts at, and
    /// `expected` details from there on.
    fn show_rows(&self, id: u64, first: u32, count: u32, expected: usize) -> (u32, Vec<Details>) {
        self.connection
            .send(Command::Rows {
                id: ListingId(id),
                first: EntryId(first),
                count,
            })
            .unwrap();
        let mut start = None;
        let mut all = Vec::new();
        while all.len() < expected {
            match self.events.recv_timeout(ANSWER_TIMEOUT).unwrap() {
                Event::DetailsRead {
                    id: ListingId(got),
                    first,
                    details,
                } if got == id => {
                    let start = *start.get_or_insert(first.0);
                    assert_eq!(
                        first.0 as usize,
                        start as usize + all.len(),
                        "runs arrive in order"
                    );
                    all.extend_from_slice(&details);
                }
                other => panic!("expected details, got {other:?}"),
            }
        }
        assert_eq!(all.len(), expected);
        (start.unwrap(), all)
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
fn details_follow_the_listing() {
    let folder = TempFolder::new();
    let root = folder.path();
    fs::create_dir(root.join("folder")).unwrap();
    fs::write(root.join("five.txt"), "12345").unwrap();
    symlink(root.join("five.txt"), root.join("link")).unwrap();
    symlink(root.join("gone"), root.join("broken")).unwrap();
    let client = client();

    let listing = listing(client.list(2, root));
    let count = listing.entries.len();
    let (_, details) = client.show_rows(2, 0, u32::try_from(count).unwrap(), count);

    let sizes: Vec<Option<u64>> = details.iter().map(|detail| detail.size).collect();
    let link_size = fs::symlink_metadata(root.join("broken")).unwrap().len();
    // folder, broken, five.txt, link
    assert_eq!(sizes, [None, Some(link_size), Some(5), Some(5)]);
    assert!(details.iter().all(|detail| detail.modified.is_some()));
}

/// A client with a listing (id 4) of a folder of `files` empty files.
fn big_folder(files: u32) -> (TempFolder, Client) {
    let folder = TempFolder::new();
    for index in 0..files {
        fs::write(folder.path().join(format!("{index:05}.txt")), "").unwrap();
    }
    let client = client();
    listing(client.list(4, folder.path()));
    (folder, client)
}

#[test]
fn only_the_rows_shown_get_details() {
    let (_folder, client) = big_folder(3000);

    let (start, details) = client.show_rows(4, 1000, 40, 40);

    // Nothing before row 1000 was read, though it was listed first.
    assert_eq!(start, 1000);
    assert!(details.iter().all(|detail| detail.size == Some(0)));
}

#[test]
fn rows_shown_before_are_not_read_again() {
    let (_folder, client) = big_folder(100);
    client.show_rows(4, 0, 10, 10);
    client.show_rows(4, 20, 10, 10);

    let (start, details) = client.show_rows(4, 0, 30, 10);

    assert_eq!(start, 10);
    assert_eq!(details.len(), 10);
}

#[test]
fn rows_past_the_end_are_left_out() {
    let (_folder, client) = big_folder(10);

    let (start, details) = client.show_rows(4, 5, 100, 5);

    assert_eq!((start, details.len()), (5, 5));
    client.connection.send(Command::Ping).unwrap();
    assert_eq!(
        client.events.recv_timeout(ANSWER_TIMEOUT).unwrap(),
        Event::Pong
    );
}

#[test]
fn rows_of_an_older_listing_are_not_read() {
    let (folder, client) = big_folder(10);
    listing(client.list(5, folder.path()));

    client
        .connection
        .send(Command::Rows {
            id: ListingId(4),
            first: EntryId(0),
            count: 10,
        })
        .unwrap();
    let (start, _) = client.show_rows(5, 3, 1, 1);

    assert_eq!(start, 3);
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
    let home = TempFolder::new();
    fs::create_dir_all(home.path().join(".config")).unwrap();
    fs::create_dir(home.path().join("Bilder")).unwrap();
    fs::create_dir(home.path().join("Downloads")).unwrap();
    fs::write(
        home.path().join(".config").join("user-dirs.dirs"),
        "XDG_PICTURES_DIR=\"$HOME/Bilder\"\n",
    )
    .unwrap();
    let client = client_at(home.path());
    client.connection.send(Command::Places).unwrap();

    let Event::Places(places) = client.events.recv_timeout(ANSWER_TIMEOUT).unwrap() else {
        panic!("expected places");
    };

    let found: Vec<(PlaceKind, PathBuf)> = places
        .into_iter()
        .map(|place| (place.kind, place.path))
        .collect();
    assert_eq!(
        found,
        [
            (PlaceKind::Home, home.path().to_owned()),
            (PlaceKind::Pictures, home.path().join("Bilder")),
            (PlaceKind::Downloads, home.path().join("Downloads")),
        ]
    );
}
