#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use image::{ImageFormat, RgbImage};
use nova_files_api::{
    ClientGone, Command, Connection, EntryId, Envelope, Event, EventSink, ListingId, Order,
    ThumbnailSize,
};
use nova_files_core::{IDLE_EXIT, RunningCore, Thumbnails};
use nova_files_thumbnails_api::Event as ThumbnailsEvent;

const HELPER: &str = env!("CARGO_BIN_EXE_nova-files-thumbnailer");
const WAIT: Duration = Duration::from_secs(5);
const WIDTH: u32 = 400;
const HEIGHT: u32 = 200;
const NORMAL_SIZE: u32 = 128;
const LARGE_SIZE: u32 = 256;
const ROWS: u32 = 10;

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

#[derive(Debug)]
enum Heard {
    Listed(Event),
    Thumbnail(ThumbnailsEvent),
}

struct Both(Sender<Heard>);

impl EventSink for Both {
    fn deliver_envelope(&self, envelope: Envelope) -> Result<(), ClientGone> {
        let heard = match envelope.open::<Event>() {
            Ok(event) => Heard::Listed(event),
            Err(other) => match other.open::<ThumbnailsEvent>() {
                Ok(event) => Heard::Thumbnail(event),
                Err(_) => return Ok(()),
            },
        };
        self.0.send(heard).map_err(|_| ClientGone)
    }
}

struct Client {
    _core: RunningCore,
    connection: Connection,
    events: Receiver<Heard>,
}

fn client(thumbnails: &Path, helper: &Path) -> Client {
    let setup = Thumbnails {
        folder: thumbnails.to_owned(),
        helper: helper.to_owned(),
    };
    let core = nova_files_core::spawn_with(IDLE_EXIT, None, Some(setup), None).unwrap();
    let (sink, events) = mpsc::channel();
    let connection = core.connector.connect(Both(sink)).unwrap();
    Client {
        _core: core,
        connection,
        events,
    }
}

fn first_thumbnail(client: &Client, folder: &Path, size: ThumbnailSize) -> (EntryId, Vec<u8>) {
    let id = ListingId(1);
    client
        .connection
        .send(Command::ListFolder {
            id,
            path: folder.to_owned(),
            order: Order::default(),
        })
        .unwrap();
    loop {
        match client.events.recv_timeout(WAIT).unwrap() {
            Heard::Listed(Event::FolderListed(listing)) if listing.id == id => break,
            Heard::Listed(Event::DetailsRead { .. }) => {}
            other => panic!("expected the listing, got {other:?}"),
        }
    }
    client
        .connection
        .send(Command::Rows {
            id,
            first: EntryId(0),
            count: ROWS,
            size,
        })
        .unwrap();
    loop {
        match client.events.recv_timeout(WAIT).unwrap() {
            Heard::Thumbnail(ThumbnailsEvent::Thumbnail {
                id: got,
                entry,
                size: made,
                png,
            }) if got == id => {
                assert_eq!(made, size);
                return (entry, png.to_vec());
            }
            Heard::Listed(Event::DetailsRead { .. }) => {}
            other => panic!("expected a thumbnail, got {other:?}"),
        }
    }
}

#[test]
fn shown_images_get_thumbnails_then_come_from_the_cache() {
    let temp = TempFolder::new("shown");
    let folder = temp.0.join("media");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("clip.mp4"), b"").unwrap();
    fs::write(folder.join("notes.txt"), b"hello").unwrap();
    RgbImage::new(WIDTH, HEIGHT)
        .save_with_format(folder.join("photo.png"), ImageFormat::Png)
        .unwrap();
    let thumbnails = temp.0.join("thumbnails");

    let normal = ThumbnailSize::Normal;
    let (entry, png) = first_thumbnail(&client(&thumbnails, Path::new(HELPER)), &folder, normal);

    assert_eq!(entry, EntryId(2));
    let image = image::load_from_memory(&png).unwrap();
    assert_eq!(
        (image.width(), image.height()),
        (NORMAL_SIZE, NORMAL_SIZE / 2)
    );
    let cached = fs::read_dir(thumbnails.join("normal")).unwrap().count();
    assert_eq!(cached, 1);

    let missing = temp.0.join("no-helper");
    let (entry, again) = first_thumbnail(&client(&thumbnails, &missing), &folder, normal);
    assert_eq!((entry, again), (EntryId(2), png));
}

#[test]
fn large_rows_get_large_thumbnails_of_their_own() {
    let temp = TempFolder::new("large");
    let folder = temp.0.join("media");
    fs::create_dir(&folder).unwrap();
    RgbImage::new(WIDTH * 2, HEIGHT * 2)
        .save_with_format(folder.join("photo.png"), ImageFormat::Png)
        .unwrap();
    let thumbnails = temp.0.join("thumbnails");
    let client = client(&thumbnails, Path::new(HELPER));

    let (_, normal) = first_thumbnail(&client, &folder, ThumbnailSize::Normal);
    let (entry, large) = first_thumbnail(&client, &folder, ThumbnailSize::Large);

    assert_eq!(entry, EntryId(0));
    let side = |png: &[u8]| {
        let image = image::load_from_memory(png).unwrap();
        (image.width(), image.height())
    };
    assert_eq!(side(&normal), (NORMAL_SIZE, NORMAL_SIZE / 2));
    assert_eq!(side(&large), (LARGE_SIZE, LARGE_SIZE / 2));
    for size in ThumbnailSize::ALL {
        let cached = fs::read_dir(thumbnails.join(size.folder()))
            .unwrap()
            .count();
        assert_eq!(cached, 1, "{size:?}");
    }
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
    let (entry, png) = first_thumbnail(&client, &folder, ThumbnailSize::Normal);

    let want = |first| {
        client
            .connection
            .send(Command::Rows {
                id: ListingId(1),
                first: EntryId(first),
                count: ROWS,
                size: ThumbnailSize::Normal,
            })
            .unwrap();
    };
    want(ROWS);
    want(0);

    loop {
        match client.events.recv_timeout(WAIT).unwrap() {
            Heard::Thumbnail(ThumbnailsEvent::Thumbnail {
                entry: got,
                png: again,
                ..
            }) => {
                assert_eq!((got, again.to_vec()), (entry, png));
                break;
            }
            Heard::Listed(Event::DetailsRead { .. }) => {}
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
    let (entry, png) = first_thumbnail(&client, &folder, ThumbnailSize::Normal);

    let missing = ListingId(2);
    client
        .connection
        .send(Command::ListFolder {
            id: missing,
            path: temp.0.join("missing"),
            order: Order::default(),
        })
        .unwrap();
    loop {
        match client.events.recv_timeout(WAIT).unwrap() {
            Heard::Listed(Event::FolderUnreadable { id, .. }) if id == missing => break,
            Heard::Listed(Event::DetailsRead { .. }) | Heard::Thumbnail(_) => {}
            other => panic!("expected the refusal, got {other:?}"),
        }
    }
    for first in [ROWS, 0] {
        client
            .connection
            .send(Command::Rows {
                id: ListingId(1),
                first: EntryId(first),
                count: ROWS,
                size: ThumbnailSize::Normal,
            })
            .unwrap();
    }

    loop {
        match client.events.recv_timeout(WAIT).unwrap() {
            Heard::Thumbnail(ThumbnailsEvent::Thumbnail {
                entry: got,
                png: again,
                ..
            }) => {
                assert_eq!((got, again.to_vec()), (entry, png));
                break;
            }
            Heard::Listed(Event::DetailsRead { .. }) => {}
            other => panic!("expected a thumbnail, got {other:?}"),
        }
    }
}

fn thumbnailed_entries(
    client: &Client,
    folder: &Path,
    id: ListingId,
    count: usize,
) -> Vec<EntryId> {
    client
        .connection
        .send(Command::ListFolder {
            id,
            path: folder.to_owned(),
            order: Order::default(),
        })
        .unwrap();
    let mut entries = Vec::new();
    while entries.len() < count {
        match client.events.recv_timeout(WAIT).unwrap() {
            Heard::Listed(Event::FolderListed(listing)) if listing.id == id => client
                .connection
                .send(Command::Rows {
                    id,
                    first: EntryId(0),
                    count: ROWS,
                    size: ThumbnailSize::Normal,
                })
                .unwrap(),
            Heard::Thumbnail(ThumbnailsEvent::Thumbnail { id: got, entry, .. }) if got == id => {
                entries.push(entry);
            }
            Heard::Listed(Event::DetailsRead { .. }) => {}
            other => panic!("expected the listing or a thumbnail, got {other:?}"),
        }
    }
    entries
}

#[test]
fn cached_rows_answer_and_rows_that_failed_before_are_not_tried_again() {
    let temp = TempFolder::new("marked");
    let folder = temp.0.join("media");
    fs::create_dir(&folder).unwrap();
    let image = RgbImage::new(WIDTH, HEIGHT);
    for name in ["a.png", "b.png"] {
        image
            .save_with_format(folder.join(name), ImageFormat::Png)
            .unwrap();
    }
    let broken = folder.join("broken.png");
    fs::write(&broken, b"not an image").unwrap();
    let thumbnails = temp.0.join("thumbnails");
    let first = thumbnailed_entries(
        &client(&thumbnails, Path::new(HELPER)),
        &folder,
        ListingId(1),
        2,
    );
    assert_eq!(first, [EntryId(0), EntryId(1)]);

    let mtime = fs::metadata(&broken).unwrap().modified().unwrap();
    image.save_with_format(&broken, ImageFormat::Png).unwrap();
    fs::File::options()
        .write(true)
        .open(&broken)
        .unwrap()
        .set_modified(mtime)
        .unwrap();
    image
        .save_with_format(folder.join("new.png"), ImageFormat::Png)
        .unwrap();

    let again = thumbnailed_entries(
        &client(&thumbnails, Path::new(HELPER)),
        &folder,
        ListingId(1),
        3,
    );
    assert_eq!(again, [EntryId(0), EntryId(1), EntryId(3)]);
}
