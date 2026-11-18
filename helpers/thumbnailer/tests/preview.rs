#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use image::{ImageFormat, Rgb, RgbImage};
use nova_files_api::{
    Command, Connection, Event, Fit, Picture, Preview, PreviewContent, PreviewId,
};
use nova_files_core::{IDLE_EXIT, RunningCore, Thumbnails};

const HELPER: &str = env!("CARGO_BIN_EXE_nova-files-thumbnailer");
const WAIT: Duration = Duration::from_secs(10);
const WIDTH: u32 = 400;
const HEIGHT: u32 = 200;
const ORANGE: [u8; 3] = [253, 112, 20];
const OPAQUE: u8 = 255;

struct TempFolder(PathBuf);

impl TempFolder {
    fn new(test: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("nova-files-preview-{test}-{}", std::process::id()));
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

impl Client {
    fn new(temp: &Path) -> Self {
        let setup = Thumbnails {
            folder: temp.join("thumbnails"),
            helper: PathBuf::from(HELPER),
        };
        let core = nova_files_core::spawn_with(IDLE_EXIT, None, Some(setup), None).unwrap();
        let (sink, events) = mpsc::channel();
        let connection = core.connector.connect(sink).unwrap();
        Self {
            _core: core,
            connection,
            events,
        }
    }

    fn preview(&self, id: u64, path: &Path, fit: Fit) -> Preview {
        self.connection
            .send(Command::Preview {
                id: PreviewId(id),
                path: path.to_owned(),
                fit,
            })
            .unwrap();
        match self.events.recv_timeout(WAIT).unwrap() {
            Event::Previewed { id: got, preview } if got == PreviewId(id) => (*preview).clone(),
            other => panic!("expected a preview, got {other:?}"),
        }
    }
}

fn picture(preview: &Preview) -> &Picture {
    match &preview.content {
        PreviewContent::Picture(picture) => picture,
        other => panic!("expected a picture, got {other:?}"),
    }
}

fn orange_picture(path: &Path, format: ImageFormat) {
    RgbImage::from_pixel(WIDTH, HEIGHT, Rgb(ORANGE))
        .save_with_format(path, format)
        .unwrap();
}

#[test]
fn a_big_picture_is_scaled_down_to_the_box() {
    let temp = TempFolder::new("down");
    let path = temp.0.join("photo.txt");
    orange_picture(&path, ImageFormat::Png);
    let client = Client::new(&temp.0);

    let shown = client.preview(
        1,
        &path,
        Fit {
            width: 100,
            height: 100,
        },
    );

    let picture = picture(&shown);
    assert_eq!((picture.width, picture.height), (100, 50));
    assert_eq!(picture.pixels.len(), 100 * 50 * 4);
    assert_eq!(
        &picture.pixels[..4],
        [ORANGE[0], ORANGE[1], ORANGE[2], OPAQUE]
    );
    assert_eq!(shown.bytes, fs::metadata(&path).unwrap().len());
}

#[test]
fn a_small_picture_keeps_its_size() {
    let temp = TempFolder::new("keeps");
    let path = temp.0.join("photo.jpg");
    orange_picture(&path, ImageFormat::Jpeg);
    let client = Client::new(&temp.0);

    let shown = client.preview(
        1,
        &path,
        Fit {
            width: WIDTH + 10,
            height: HEIGHT + 10,
        },
    );

    let picture = picture(&shown);
    assert_eq!((picture.width, picture.height), (WIDTH, HEIGHT));
}

#[test]
fn a_broken_picture_shows_its_icon() {
    let temp = TempFolder::new("broken");
    let path = temp.0.join("photo.png");
    fs::write(&path, b"\x89PNG\r\n\x1a\nnot really").unwrap();
    let client = Client::new(&temp.0);

    let shown = client.preview(
        1,
        &path,
        Fit {
            width: 100,
            height: 100,
        },
    );

    assert_eq!(shown.content, PreviewContent::Icon);
}

#[test]
fn stepping_through_pictures_answers_the_last() {
    let temp = TempFolder::new("stepping");
    let paths: Vec<PathBuf> = (0..5)
        .map(|n| {
            let path = temp.0.join(format!("photo-{n}.png"));
            orange_picture(&path, ImageFormat::Png);
            path
        })
        .collect();
    let client = Client::new(&temp.0);
    let fit = Fit {
        width: 100,
        height: 100,
    };
    for (id, path) in (1..).zip(&paths) {
        client
            .connection
            .send(Command::Preview {
                id: PreviewId(id),
                path: path.clone(),
                fit,
            })
            .unwrap();
    }
    let last = PreviewId(5);
    loop {
        match client.events.recv_timeout(WAIT).unwrap() {
            Event::Previewed { id, preview } if id == last => {
                assert_eq!(preview.path, paths[4]);
                break;
            }
            Event::Previewed { id, .. } => assert!(id < last),
            other => panic!("expected a preview, got {other:?}"),
        }
    }
}

#[test]
fn a_picture_too_wide_for_the_box_is_scaled_down() {
    let temp = TempFolder::new("wide");
    let path = temp.0.join("photo.png");
    orange_picture(&path, ImageFormat::Png);
    let client = Client::new(&temp.0);

    let shown = client.preview(
        1,
        &path,
        Fit {
            width: 100,
            height: 4000,
        },
    );

    let picture = picture(&shown);
    assert_eq!((picture.width, picture.height), (100, 50));
}

#[test]
fn a_text_file_is_not_sent_to_the_helper() {
    let temp = TempFolder::new("text");
    let path = temp.0.join("notes.png");
    fs::write(&path, "plain words").unwrap();
    let client = Client::new(&temp.0);

    let shown = client.preview(
        1,
        &path,
        Fit {
            width: 100,
            height: 100,
        },
    );

    assert_eq!(
        shown.content,
        PreviewContent::Text {
            text: "plain words".into(),
            cut: false
        }
    );
}
