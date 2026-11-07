//! The core and the real helper together: a client lists a folder, asks for
//! thumbnails of the rows it shows, and gets them, made by the helper the
//! first time and read from the shared cache after that.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use image::{ImageFormat, RgbImage};
use nova_files_api::{Command, Connection, EntryId, Event, ListingId};
use nova_files_core::{IDLE_EXIT, RunningCore, Thumbnails};

const HELPER: &str = env!("CARGO_BIN_EXE_nova-files-thumbnailer");
const WAIT: Duration = Duration::from_secs(10);
const WIDTH: u32 = 400;
const HEIGHT: u32 = 200;
const NORMAL_SIZE: u32 = 128;
const ROWS: u32 = 10;

/// A folder under the system temp folder, removed when dropped.
struct TempFolder(PathBuf);

impl TempFolder {
    fn new(test: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "nova-files-thumbs-core-{test}-{}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

struct Client {
    _core: RunningCore,
    connection: Connection,
    events: Receiver<Event>,
}

fn client(thumbnails: &Path, helper: &Path) -> Client {
    let setup = Thumbnails {
        folder: thumbnails.to_owned(),
        helper: helper.to_owned(),
    };
    let core = nova_files_core::spawn_with(IDLE_EXIT, None, Some(setup)).unwrap();
    let (sink, events) = mpsc::channel();
    let connection = core.connector.connect(sink).unwrap();
    Client {
        _core: core,
        connection,
        events,
    }
}

/// Lists `folder`, asks for the first rows' thumbnails and returns the
/// first thumbnail that arrives.
fn first_thumbnail(client: &Client, folder: &Path) -> (EntryId, Vec<u8>) {
    let id = ListingId(1);
    client
        .connection
        .send(Command::ListFolder {
            id,
            path: folder.to_owned(),
        })
        .unwrap();
    loop {
        match client.events.recv_timeout(WAIT).unwrap() {
            Event::FolderListed(listing) if listing.id == id => break,
            Event::DetailsRead { .. } => {}
            other => panic!("expected the listing, got {other:?}"),
        }
    }
    client
        .connection
        .send(Command::Thumbnails {
            id,
            first: EntryId(0),
            count: ROWS,
        })
        .unwrap();
    loop {
        match client.events.recv_timeout(WAIT).unwrap() {
            Event::Thumbnail {
                id: got,
                entry,
                png,
            } if got == id => return (entry, png.to_vec()),
            Event::DetailsRead { .. } => {}
            other => panic!("expected a thumbnail, got {other:?}"),
        }
    }
}

#[test]
fn shown_images_get_thumbnails_then_come_from_the_cache() {
    let temp = TempFolder::new("shown");
    let folder = temp.0.join("media");
    fs::create_dir(&folder).unwrap();
    // Sorted: clip.mp4, notes.txt, photo.png.
    fs::write(folder.join("clip.mp4"), b"").unwrap();
    fs::write(folder.join("notes.txt"), b"hello").unwrap();
    RgbImage::new(WIDTH, HEIGHT)
        .save_with_format(folder.join("photo.png"), ImageFormat::Png)
        .unwrap();
    let thumbnails = temp.0.join("thumbnails");

    let (entry, png) = first_thumbnail(&client(&thumbnails, Path::new(HELPER)), &folder);

    assert_eq!(entry, EntryId(2));
    let image = image::load_from_memory(&png).unwrap();
    assert_eq!(
        (image.width(), image.height()),
        (NORMAL_SIZE, NORMAL_SIZE / 2)
    );
    let cached = fs::read_dir(thumbnails.join("normal")).unwrap().count();
    assert_eq!(cached, 1);

    // A core whose helper is missing can only answer from the cache.
    let missing = temp.0.join("no-helper");
    let (entry, again) = first_thumbnail(&client(&thumbnails, &missing), &folder);
    assert_eq!((entry, again), (EntryId(2), png));
}

#[test]
fn rows_scrolled_away_and_back_get_their_thumbnails_again() {
    let temp = TempFolder::new("back");
    let folder = temp.0.join("media");
    fs::create_dir(&folder).unwrap();
    RgbImage::new(WIDTH, HEIGHT)
        .save_with_format(folder.join("photo.png"), ImageFormat::Png)
        .unwrap();
    let client = client(&temp.0.join("thumbnails"), Path::new(HELPER));
    let (entry, png) = first_thumbnail(&client, &folder);

    let want = |first| {
        client
            .connection
            .send(Command::Thumbnails {
                id: ListingId(1),
                first: EntryId(first),
                count: ROWS,
            })
            .unwrap();
    };
    // Away, past the photo, then back.
    want(ROWS);
    want(0);

    loop {
        match client.events.recv_timeout(WAIT).unwrap() {
            Event::Thumbnail {
                entry: got,
                png: again,
                ..
            } => {
                assert_eq!((got, again.to_vec()), (entry, png));
                break;
            }
            Event::DetailsRead { .. } => {}
            other => panic!("expected a thumbnail, got {other:?}"),
        }
    }
}

#[test]
fn a_refused_folder_leaves_the_shown_one_its_thumbnails() {
    let temp = TempFolder::new("refused");
    let folder = temp.0.join("media");
    fs::create_dir(&folder).unwrap();
    RgbImage::new(WIDTH, HEIGHT)
        .save_with_format(folder.join("photo.png"), ImageFormat::Png)
        .unwrap();
    let client = client(&temp.0.join("thumbnails"), Path::new(HELPER));
    let (entry, png) = first_thumbnail(&client, &folder);

    let missing = ListingId(2);
    client
        .connection
        .send(Command::ListFolder {
            id: missing,
            path: temp.0.join("missing"),
        })
        .unwrap();
    loop {
        match client.events.recv_timeout(WAIT).unwrap() {
            Event::FolderUnreadable { id, .. } if id == missing => break,
            Event::DetailsRead { .. } | Event::Thumbnail { .. } => {}
            other => panic!("expected the refusal, got {other:?}"),
        }
    }
    // Still on the first listing: away and back again.
    for first in [ROWS, 0] {
        client
            .connection
            .send(Command::Thumbnails {
                id: ListingId(1),
                first: EntryId(first),
                count: ROWS,
            })
            .unwrap();
    }

    loop {
        match client.events.recv_timeout(WAIT).unwrap() {
            Event::Thumbnail {
                entry: got,
                png: again,
                ..
            } => {
                assert_eq!((got, again.to_vec()), (entry, png));
                break;
            }
            Event::DetailsRead { .. } => {}
            other => panic!("expected a thumbnail, got {other:?}"),
        }
    }
}
