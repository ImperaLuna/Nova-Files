use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt as _;
use std::os::unix::fs::{PermissionsExt as _, symlink};
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use nova_files_api::{
    Command, Connection, Details, EntryId, EntryKind, Event, Listing, ListingId, Order,
    ThumbnailSize, Unreadable,
};
use nova_files_test_folder::TestFolder;

use crate::common::Core;

const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);
const NO_ACCESS: u32 = 0o000;

struct Client {
    _core: Core,
    connection: Connection,
    events: Receiver<Event>,
}

fn client_at(home: &Path) -> Client {
    let home = nova_files::Home {
        folder: home.to_owned(),
        config: home.join(".config"),
        state: home.join(".local/state"),
        data: home.join(".local/share"),
        drive_trashes: false,
    };
    let core = Core(nova_files::spawn_with(nova_files::IDLE_EXIT, Some(home), None, None).unwrap());
    let (sink, events) = mpsc::channel();
    let connection = core.connector.connect(sink).unwrap();
    Client {
        _core: core,
        connection,
        events,
    }
}

fn client() -> Client {
    let home = TestFolder::new("core-test");
    let client = client_at(home.path());
    drop(home);
    client
}

impl Client {
    fn list(&self, id: u64, path: &Path) -> Event {
        self.connection
            .send(Command::ListFolder {
                id: ListingId(id),
                path: path.to_owned(),
                order: Order::default(),
                filter: String::new(),
            })
            .unwrap();
        self.events.recv_timeout(ANSWER_TIMEOUT).unwrap()
    }

    fn show_rows(&self, id: u64, first: u32, count: u32, expected: usize) -> (u32, Vec<Details>) {
        self.connection
            .send(Command::Rows {
                id: ListingId(id),
                first: EntryId(first),
                count,
                size: ThumbnailSize::Normal,
                peeks: false,
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
                    ..
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
    let folder = TestFolder::new("core-test");
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
    let folder = TestFolder::new("core-test");
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
    assert_eq!(sizes, [None, Some(link_size), Some(5), Some(5)]);
    assert!(details.iter().all(|detail| detail.modified.is_some()));
}

fn big_folder(files: u32) -> (TestFolder, Client) {
    let folder = TestFolder::new("core-test");
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
            size: ThumbnailSize::Normal,
            peeks: false,
        })
        .unwrap();
    let (start, _) = client.show_rows(5, 3, 1, 1);

    assert_eq!(start, 3);
}

#[test]
fn hidden_files_are_left_out_unless_the_settings_show_them() {
    let folder = TestFolder::new("core-test");
    fs::write(folder.path().join(".hidden"), "").unwrap();
    fs::write(folder.path().join("shown"), "").unwrap();

    let left_out = listing(client().list(1, folder.path()));
    assert_eq!(names(&left_out), [("shown".to_owned(), EntryKind::File)]);
    assert_eq!(left_out.entries[0].id, EntryId(0));

    let home = TestFolder::new("core-test");
    let config = home.path().join(".config/nova-files");
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("settings.toml"), "hidden_files = true\n").unwrap();
    let shown = listing(client_at(home.path()).list(1, folder.path()));
    assert_eq!(
        names(&shown),
        [
            (".hidden".to_owned(), EntryKind::File),
            ("shown".to_owned(), EntryKind::File)
        ]
    );
}

#[test]
fn the_trash_lists_its_hidden_items_too() {
    let home = TestFolder::new("core-test");
    let trash = home.path().join(".local/share/Trash/files");
    fs::create_dir_all(trash.join(".old")).unwrap();
    fs::write(trash.join(".env"), "").unwrap();
    let client = client_at(home.path());

    let trashed = listing(client.list(1, &trash));
    assert_eq!(
        names(&trashed),
        [
            (".old".to_owned(), EntryKind::Folder),
            (".env".to_owned(), EntryKind::File)
        ]
    );
    fs::write(trash.join(".old/.inside"), "").unwrap();
    let inside = listing(client.list(2, &trash.join(".old")));
    assert_eq!(names(&inside), [(".inside".to_owned(), EntryKind::File)]);
}

#[test]
fn empty_folder_lists_nothing() {
    let folder = TestFolder::new("core-test");

    let listing = listing(client().list(1, folder.path()));

    assert_eq!(listing.entries.len(), 0);
}

#[test]
fn name_that_is_not_utf8_comes_back_unchanged() {
    let folder = TestFolder::new("core-test");
    let name = OsString::from_vec(vec![b'b', 0xff, b'd']);
    fs::write(folder.path().join(&name), "").unwrap();

    let listing = listing(client().list(1, folder.path()));

    assert_eq!(listing.entries.len(), 1);
    assert_eq!(listing.entries[0].name, name);
}

#[test]
fn missing_folder_is_reported_missing() {
    let folder = TestFolder::new("core-test");
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
    let folder = TestFolder::new("core-test");
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
    let folder = TestFolder::new("core-test");
    let locked = folder.path().join("locked");
    fs::create_dir(&locked).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(NO_ACCESS)).unwrap();
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
