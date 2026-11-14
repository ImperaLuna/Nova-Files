//! A window on a real core and the real helper: an image row in the list
//! gets its thumbnail once the list has drawn it, and the quick look shows
//! the picture itself.

#![expect(
    clippy::future_not_send,
    clippy::unwrap_used,
    reason = "GPUI test contexts live on one thread; test setup should fail loudly"
)]

use std::fs;
use std::io::{self, Write as _};
use std::path::PathBuf;
use std::process;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
use image::{ImageFormat, RgbImage};
use nova_files_api::PreviewContent;
use nova_files_core::{IDLE_EXIT, Thumbnails};
use nova_files_ui::{MainWindow, WindowEvent};

const HELPER: &str = env!("CARGO_BIN_EXE_nova-files-thumbnailer");
const SIDE: u32 = 300;

/// How long the test may wait for the core and the helper.
const WAIT: Duration = Duration::from_secs(5);

/// Fails the test process if the test is still running after [`WAIT`]:
/// GPUI's waits have no deadline of their own while the core answers from
/// real threads, so a thumbnail that never comes would hang.
struct Watchdog(Sender<()>);

impl Watchdog {
    fn start() -> Self {
        let (done, finished) = mpsc::channel();
        thread::spawn(move || {
            if finished.recv_timeout(WAIT) == Err(RecvTimeoutError::Timeout) {
                writeln!(io::stderr(), "no thumbnail within {WAIT:?}").ok();
                process::exit(1);
            }
        });
        Self(done)
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.0.send(()).ok();
    }
}

/// A folder under the system temp folder, removed when dropped.
struct TempFolder(PathBuf);

impl Drop for TempFolder {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

/// A window with the real helper on a folder holding `notes.txt` and
/// `photo.jpg`, in that order, once that folder is on screen.
async fn window_on_media<'a>(
    cx: &'a mut TestAppContext,
    test: &str,
) -> (
    TempFolder,
    Entity<MainWindow>,
    &'a mut VisualTestContext,
    nova_files_core::RunningCore,
) {
    let temp = TempFolder(
        std::env::temp_dir().join(format!("nova-files-thumbs-window-{test}-{}", process::id())),
    );
    let folder = temp.0.join("media");
    fs::create_dir_all(&folder).unwrap();
    // Sorted: notes.txt, photo.jpg.
    fs::write(folder.join("notes.txt"), b"hello").unwrap();
    RgbImage::new(SIDE, SIDE)
        .save_with_format(folder.join("photo.jpg"), ImageFormat::Jpeg)
        .unwrap();
    let setup = Thumbnails {
        folder: temp.0.join("thumbnails"),
        helper: PathBuf::from(HELPER),
    };
    // The core and the helper answer from their own threads.
    cx.executor().allow_parking();
    let core = nova_files_core::spawn_with(IDLE_EXIT, None, Some(setup), None).unwrap();
    let (start, home) = (folder.clone(), temp.0.clone());
    let (view, cx) =
        cx.add_window_view(|_, cx| MainWindow::with_home(&core.connector, &start, home, cx));

    view.condition::<WindowEvent>(cx, |view, _| view.folder() == Some(folder.as_path()))
        .await;
    cx.run_until_parked();
    (temp, view, cx, core)
}

#[gpui::test]
async fn an_image_row_gets_its_thumbnail(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let (_temp, view, cx, _core) = window_on_media(cx, "thumbnail").await;

    view.condition::<WindowEvent>(cx, |view, _| view.thumbnail(1).is_some())
        .await;

    view.read_with(cx, |view, _| assert!(view.thumbnail(0).is_none()));
}

#[gpui::test]
async fn the_quick_look_shows_the_picture_itself(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    cx.update(nova_files_ui::bind_window_keys);
    let (_temp, view, cx, _core) = window_on_media(cx, "preview").await;

    let row = cx.debug_bounds("entry-1").unwrap();
    cx.simulate_click(row.center(), Modifiers::none());
    cx.simulate_keystrokes("space");
    view.condition::<WindowEvent>(cx, |view, _| {
        view.previewed().is_some_and(|shown| shown.picture)
    })
    .await;
    let preview = view.read_with(cx, |view, _| view.previewed().unwrap().preview.unwrap());
    match &preview.content {
        PreviewContent::Picture(picture) => {
            assert_eq!((picture.width, picture.height), (SIDE, SIDE));
        }
        other => unreachable!("expected a picture, got {other:?}"),
    }

    cx.simulate_keystrokes("space");
    view.read_with(cx, |view, _| assert!(!view.previewing()));
}
