#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use image::codecs::gif::GifEncoder;
use image::{Delay, Frame, Rgba, RgbaImage};
use nova_files::{IDLE_EXIT, RunningCore, Thumbnails};
use nova_files_api::{Connection, Fit};
use nova_files_preview_api::{Command, Control, Event, Played, PreviewContent, PreviewId};
use nova_files_test_folder::TestFolder;

const PLAYER: &str = env!("CARGO_BIN_EXE_nova-files-player");
const WAIT: Duration = Duration::from_secs(10);
const FRAMES: u8 = 10;
const FRAME_MS: u32 = 100;
const SIDE: u32 = 64;
const FIT: Fit = Fit {
    width: 32,
    height: 32,
};
const SMALLER: Fit = Fit {
    width: 16,
    height: 16,
};
const ID: PreviewId = PreviewId(1);

struct Client {
    _core: RunningCore,
    connection: Connection,
    events: Receiver<Event>,
}

impl Client {
    fn new(temp: &Path) -> Self {
        let setup = Thumbnails {
            folder: temp.join("thumbnails"),
            helper: Path::new(PLAYER).with_file_name("nova-files-thumbnailer"),
        };
        let core = nova_files::spawn_with(IDLE_EXIT, None, Some(setup), None).unwrap();
        let (sink, events) = mpsc::channel();
        let connection = core.connector.connect(sink).unwrap();
        Self {
            _core: core,
            connection,
            events,
        }
    }

    fn played_until(&self, mut wanted: impl FnMut(&Played) -> bool) -> Vec<Played> {
        let deadline = Instant::now() + WAIT;
        let mut seen = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left).unwrap() {
                Event::Played { id, played } if id == ID => {
                    assert_ne!(played, Played::Unplayable, "libmpv must be installed");
                    let done = wanted(&played);
                    seen.push(played);
                    if done {
                        return seen;
                    }
                }
                other => panic!("expected playing, got {other:?}"),
            }
        }
    }

    fn control(&self, control: Control) {
        self.connection
            .send(Command::Control { id: ID, control })
            .unwrap();
    }
}

fn clip(folder: &TestFolder) -> PathBuf {
    let path = folder.join("clip.mp4");
    let mut encoder = GifEncoder::new(File::create(&path).unwrap());
    for shade in 0..FRAMES {
        let image = RgbaImage::from_pixel(SIDE, SIDE, Rgba([shade * 20, 90, 200, 255]));
        let delay = Delay::from_numer_denom_ms(FRAME_MS, 1);
        encoder
            .encode_frame(Frame::from_parts(image, 0, 0, delay))
            .unwrap();
    }
    drop(encoder);
    path
}

fn start(client: &Client, path: &Path) {
    client
        .connection
        .send(Command::Preview {
            id: ID,
            path: path.to_owned(),
            fit: FIT,
        })
        .unwrap();
    match client.events.recv_timeout(WAIT).unwrap() {
        Event::Previewed { id, preview } if id == ID => {
            assert_eq!(preview.content, PreviewContent::Media { video: true });
        }
        other => panic!("expected a preview, got {other:?}"),
    }
}

#[test]
fn a_video_plays_with_frames_fitted_to_the_box_and_its_length() {
    let temp = TestFolder::new("play-video");
    let path = clip(&temp);
    let client = Client::new(temp.path());

    start(&client, &path);

    let seen = client.played_until(|played| matches!(played, Played::Ended));
    let frame = seen.iter().find_map(|played| match played {
        Played::Frame(frame) => Some(frame),
        _ => None,
    });
    let frame = frame.unwrap();
    assert_eq!((frame.width, frame.height), (FIT.width, FIT.height));
    assert_eq!(frame.pixels[3], 255);
    let length = seen.iter().find_map(|played| match played {
        Played::Length(length) => Some(*length),
        _ => None,
    });
    let length = length.unwrap();
    assert!(
        (Duration::from_millis(900)..=Duration::from_millis(1100)).contains(&length),
        "{length:?}"
    );
}

#[test]
fn pausing_and_seeking_are_reported_back() {
    let temp = TestFolder::new("play-seek");
    let path = clip(&temp);
    let client = Client::new(temp.path());
    start(&client, &path);
    client.played_until(|played| matches!(played, Played::Frame(_)));

    client.control(Control::Pause);
    client.played_until(|played| *played == Played::Paused(true));
    client.control(Control::Seek(Duration::from_millis(700)));
    client.played_until(
        |played| matches!(played, Played::At(at) if *at >= Duration::from_millis(600)),
    );
    client.control(Control::Play);
    client.played_until(|played| *played == Played::Paused(false));
}

#[test]
fn a_paused_video_is_drawn_again_at_a_new_size() {
    let temp = TestFolder::new("play-resize");
    let path = clip(&temp);
    let client = Client::new(temp.path());
    start(&client, &path);
    client.played_until(|played| matches!(played, Played::Frame(_)));
    client.control(Control::Pause);
    client.played_until(|played| *played == Played::Paused(true));

    client.control(Control::Fit(SMALLER));

    client.played_until(|played| {
        matches!(played, Played::Frame(frame) if (frame.width, frame.height) == (SMALLER.width, SMALLER.height))
    });
}
