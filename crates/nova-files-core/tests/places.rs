//! The places list through the real core, in a temp home: the standard
//! folders that exist, the pinned folders from the shared bookmarks file,
//! both followed on disk, and pins changed by a client.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

use std::fs;
use std::os::unix::fs::{PermissionsExt as _, symlink};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use nova_files_api::{Bookmark, BookmarkChange, Command, Connection, Event, Place, PlaceKind};
use nova_files_core::{Home, IDLE_EXIT, RunningCore};

/// How long a test waits for the core before it counts as a failure.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);
/// Long enough for the watcher's quiet wait and a read: no event in this
/// time means none is coming.
const NOTHING_MORE: Duration = Duration::from_millis(600);
const SHARED_MODE: u32 = 0o644;
const PERMISSION_BITS: u32 = 0o7777;
/// One byte over the core's cap for the bookmarks file.
const TOO_BIG: usize = 1_048_577;

struct TempHome(PathBuf);

impl TempHome {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("nova-files-places-{name}-{}", std::process::id()));
        fs::remove_dir_all(&path).ok();
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn at(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn bookmarks(&self) -> PathBuf {
        self.at(".config/gtk-3.0/bookmarks")
    }

    /// Writes the bookmarks file the way an editor or another app does: a
    /// new file renamed over the old one.
    fn write_bookmarks(&self, text: &str) {
        let file = self.bookmarks();
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let temporary = file.with_extension("tmp");
        fs::write(&temporary, text).unwrap();
        fs::rename(temporary, file).unwrap();
    }

    fn core(&self) -> RunningCore {
        let home = Home {
            folder: self.0.clone(),
            config: self.at(".config"),
            state: self.at(".local/state"),
            data: self.at(".local/share"),
        };
        nova_files_core::spawn_with(IDLE_EXIT, Some(home), None, None).unwrap()
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

struct Client {
    connection: Connection,
    events: Receiver<Event>,
}

impl Client {
    fn of(core: &RunningCore) -> Self {
        let (sink, events) = mpsc::channel();
        let connection = core.connector.connect(sink).unwrap();
        Self { connection, events }
    }

    /// Asks for the places and returns the first answer.
    fn ask(&self) -> (Vec<Place>, Vec<Bookmark>) {
        self.connection.send(Command::Places).unwrap();
        (self.places(), self.bookmarks())
    }

    fn change(&self, change: BookmarkChange) {
        self.connection
            .send(Command::ChangeBookmark(change))
            .unwrap();
    }

    fn places(&self) -> Vec<Place> {
        match self.events.recv_timeout(ANSWER_TIMEOUT).unwrap() {
            Event::Places(places) => places,
            other => panic!("expected the places, got {other:?}"),
        }
    }

    fn bookmarks(&self) -> Vec<Bookmark> {
        match self.events.recv_timeout(ANSWER_TIMEOUT).unwrap() {
            Event::Bookmarks(bookmarks) => bookmarks.to_vec(),
            other => panic!("expected the pinned folders, got {other:?}"),
        }
    }

    fn nothing_more(&self) {
        if let Ok(event) = self.events.recv_timeout(NOTHING_MORE) {
            panic!("expected nothing more, got {event:?}");
        }
    }
}

fn kinds(places: &[Place]) -> Vec<(PlaceKind, PathBuf)> {
    places
        .iter()
        .map(|place| (place.kind, place.path.clone()))
        .collect()
}

fn pinned(path: &Path, label: Option<&str>) -> Bookmark {
    Bookmark {
        path: path.to_owned(),
        label: label.map(str::to_owned),
    }
}

#[test]
fn places_lists_existing_standard_folders_in_order() {
    let home = TempHome::new("standard");
    fs::create_dir_all(home.at(".config")).unwrap();
    fs::create_dir(home.at("Bilder")).unwrap();
    fs::create_dir(home.at("Downloads")).unwrap();
    fs::create_dir(home.at("Videos")).unwrap();
    // A file with a folder's name isn't one.
    fs::write(home.at("Music"), b"").unwrap();
    fs::write(
        home.at(".config/user-dirs.dirs"),
        "XDG_PICTURES_DIR=\"$HOME/Bilder\"\n",
    )
    .unwrap();
    let core = home.core();

    let (places, bookmarks) = Client::of(&core).ask();

    assert_eq!(
        kinds(&places),
        [
            (PlaceKind::Home, home.0.clone()),
            (PlaceKind::Downloads, home.at("Downloads")),
            (PlaceKind::Pictures, home.at("Bilder")),
            (PlaceKind::Videos, home.at("Videos")),
        ]
    );
    assert_eq!(bookmarks, []);
}

#[test]
fn a_standard_folder_made_or_removed_is_sent_again() {
    let home = TempHome::new("follow");
    let core = home.core();
    let client = Client::of(&core);
    let (places, _) = client.ask();
    assert_eq!(kinds(&places), [(PlaceKind::Home, home.0.clone())]);

    fs::create_dir(home.at("Music")).unwrap();
    assert_eq!(
        kinds(&client.places()),
        [
            (PlaceKind::Home, home.0.clone()),
            (PlaceKind::Music, home.at("Music")),
        ]
    );

    // Other files in Home send nothing.
    fs::write(home.at("notes.txt"), b"x").unwrap();
    client.nothing_more();

    fs::remove_dir(home.at("Music")).unwrap();
    assert_eq!(kinds(&client.places()), [(PlaceKind::Home, home.0.clone())]);
}

#[test]
fn moving_a_standard_folder_in_user_dirs_is_sent_again() {
    let home = TempHome::new("user-dirs");
    fs::create_dir_all(home.at(".config")).unwrap();
    fs::create_dir(home.at("Desktop")).unwrap();
    fs::create_dir(home.at("Schreibtisch")).unwrap();
    let core = home.core();
    let client = Client::of(&core);
    let (places, _) = client.ask();
    assert_eq!(places[1].path, home.at("Desktop"));

    fs::write(
        home.at(".config/user-dirs.dirs"),
        "XDG_DESKTOP_DIR=\"$HOME/Schreibtisch\"\n",
    )
    .unwrap();

    assert_eq!(
        kinds(&client.places()),
        [
            (PlaceKind::Home, home.0.clone()),
            (PlaceKind::Desktop, home.at("Schreibtisch")),
        ]
    );
}

#[test]
fn pinned_folders_come_from_the_shared_bookmarks_file() {
    let home = TempHome::new("read");
    home.write_bookmarks(
        "file:///srv/Work Work stuff\nsftp://host/x Server\nfile:///home/x/My%20Folder\n",
    );
    let core = home.core();

    let (_, bookmarks) = Client::of(&core).ask();

    assert_eq!(
        bookmarks,
        [
            pinned(Path::new("/srv/Work"), Some("Work stuff")),
            pinned(Path::new("/home/x/My Folder"), None),
        ]
    );
}

#[test]
fn bookmarks_edited_by_another_app_are_sent_again() {
    let home = TempHome::new("edited");
    let core = home.core();
    let client = Client::of(&core);
    assert_eq!(client.ask().1, []);

    home.write_bookmarks("file:///a A\n");

    assert_eq!(client.bookmarks(), [pinned(Path::new("/a"), Some("A"))]);
    // The same pins again send nothing.
    home.write_bookmarks("file:///a A\n");
    client.nothing_more();
}

#[test]
fn a_bookmarks_file_that_cant_be_read_keeps_the_pins_last_read() {
    let home = TempHome::new("unreadable");
    home.write_bookmarks("file:///a\n");
    let core = home.core();
    let client = Client::of(&core);
    assert_eq!(client.ask().1, [pinned(Path::new("/a"), None)]);

    // Over the cap: not a bookmarks file anyone wrote by hand.
    home.write_bookmarks(&"x".repeat(TOO_BIG));
    client.nothing_more();
    // Nor is it saved over.
    client.change(BookmarkChange::Add {
        path: PathBuf::from("/b"),
        label: None,
        at: None,
    });
    client.nothing_more();
    assert_eq!(
        fs::metadata(home.bookmarks()).unwrap().len(),
        TOO_BIG as u64
    );

    home.write_bookmarks("file:///c\n");
    assert_eq!(client.bookmarks(), [pinned(Path::new("/c"), None)]);
}

#[test]
fn a_client_pins_renames_moves_and_unpins_and_every_listener_hears() {
    let home = TempHome::new("change");
    home.write_bookmarks("sftp://host/x Server\nfile:///a\n");
    fs::set_permissions(home.bookmarks(), fs::Permissions::from_mode(SHARED_MODE)).unwrap();
    let core = home.core();
    let client = Client::of(&core);
    let other = Client::of(&core);
    client.ask();
    other.ask();

    client.change(BookmarkChange::Add {
        path: PathBuf::from("/b"),
        label: Some("Bee".to_owned()),
        at: Some(0),
    });
    let expected = [
        pinned(Path::new("/b"), Some("Bee")),
        pinned(Path::new("/a"), None),
    ];
    assert_eq!(client.bookmarks(), expected);
    assert_eq!(other.bookmarks(), expected);

    client.change(BookmarkChange::Rename {
        path: PathBuf::from("/a"),
        label: Some("Ay".to_owned()),
    });
    client.bookmarks();
    client.change(BookmarkChange::Move {
        path: PathBuf::from("/b"),
        to: 1,
    });
    assert_eq!(
        client.bookmarks(),
        [
            pinned(Path::new("/a"), Some("Ay")),
            pinned(Path::new("/b"), Some("Bee")),
        ]
    );
    assert_eq!(
        fs::read_to_string(home.bookmarks()).unwrap(),
        "sftp://host/x Server\nfile:///a Ay\nfile:///b Bee\n"
    );
    let mode = fs::metadata(home.bookmarks()).unwrap().permissions().mode();
    assert_eq!(mode & PERMISSION_BITS, SHARED_MODE);

    client.change(BookmarkChange::Remove {
        path: PathBuf::from("/a"),
    });
    assert_eq!(client.bookmarks(), [pinned(Path::new("/b"), Some("Bee"))]);
    // A change that changes nothing sends nothing.
    client.change(BookmarkChange::Remove {
        path: PathBuf::from("/a"),
    });
    client.nothing_more();
}

#[test]
fn the_first_pin_makes_the_file_and_a_linked_file_stays_linked() {
    let home = TempHome::new("new-file");
    let core = home.core();
    let client = Client::of(&core);
    client.ask();
    client.change(BookmarkChange::Add {
        path: PathBuf::from("/first"),
        label: None,
        at: None,
    });
    assert_eq!(client.bookmarks(), [pinned(Path::new("/first"), None)]);
    assert_eq!(
        fs::read_to_string(home.bookmarks()).unwrap(),
        "file:///first\n"
    );

    // A bookmarks file kept elsewhere (dotfiles) and linked in.
    let kept = home.at("dotfiles-bookmarks");
    fs::rename(home.bookmarks(), &kept).unwrap();
    symlink(&kept, home.bookmarks()).unwrap();
    client.change(BookmarkChange::Add {
        path: PathBuf::from("/second"),
        label: None,
        at: None,
    });
    loop {
        if client.bookmarks().len() == 2 {
            break;
        }
    }
    assert!(
        fs::symlink_metadata(home.bookmarks())
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read_to_string(&kept).unwrap(),
        "file:///first\nfile:///second\n"
    );
}

#[test]
fn a_client_that_never_asks_hears_nothing() {
    let home = TempHome::new("quiet");
    let core = home.core();
    let quiet = Client::of(&core);
    let asking = Client::of(&core);
    asking.ask();

    asking.change(BookmarkChange::Add {
        path: PathBuf::from("/a"),
        label: None,
        at: None,
    });
    asking.bookmarks();
    fs::create_dir(home.at("Videos")).unwrap();
    asking.places();

    quiet.nothing_more();
}
