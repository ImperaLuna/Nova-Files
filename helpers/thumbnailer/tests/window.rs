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

use gpui::{App, Entity, Modifiers, TestAppContext, VisualTestContext};
use image::{ImageFormat, RgbImage};
use nova_files::{IDLE_EXIT, Thumbnails};
use nova_files_preview_api::PreviewContent;
use nova_files_preview_ui::{PreviewPart, Previewed};
use nova_files_shell::Parts;
use nova_files_thumbnails_ui::ThumbnailsPart;
use nova_files_window::{MainWindow, WindowEvent};

const HELPER: &str = env!("CARGO_BIN_EXE_nova-files-thumbnailer");
const SIDE: u32 = 300;

const WAIT: Duration = Duration::from_secs(5);

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

struct TempFolder(PathBuf);

impl Drop for TempFolder {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

async fn window_on_media<'a>(
    cx: &'a mut TestAppContext,
    test: &str,
) -> (
    TempFolder,
    Entity<MainWindow>,
    &'a mut VisualTestContext,
    nova_files::RunningCore,
) {
    let temp = TempFolder(
        std::env::temp_dir().join(format!("nova-files-thumbs-window-{test}-{}", process::id())),
    );
    let folder = temp.0.join("media");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("notes.txt"), b"hello").unwrap();
    RgbImage::new(SIDE, SIDE)
        .save_with_format(folder.join("photo.jpg"), ImageFormat::Jpeg)
        .unwrap();
    let setup = Thumbnails {
        folder: temp.0.join("thumbnails"),
        helper: PathBuf::from(HELPER),
    };
    cx.executor().allow_parking();
    cx.update(|cx| {
        let mut parts = Parts::default();
        parts.add::<PreviewPart>();
        parts.add::<ThumbnailsPart>();
        cx.set_global(parts);
    });
    let core = nova_files::spawn_with(IDLE_EXIT, None, Some(setup), None).unwrap();
    let (start, home) = (folder.clone(), temp.0.clone());
    let (view, cx) =
        cx.add_window_view(|_, cx| MainWindow::with_home(&core.connector, &start, home, cx));

    view.condition::<WindowEvent>(cx, |view, _| view.folder() == Some(folder.as_path()))
        .await;
    cx.run_until_parked();
    (temp, view, cx, core)
}

fn previewing(view: &MainWindow, cx: &App) -> bool {
    view.part::<PreviewPart>()
        .is_some_and(|part| part.read(cx).previewing())
}

fn previewed(view: &MainWindow, cx: &App) -> Option<Previewed> {
    view.part::<PreviewPart>()?.read(cx).previewed(cx)
}

fn has_thumbnail(view: &MainWindow, index: usize, cx: &App) -> bool {
    view.part::<ThumbnailsPart>()
        .is_some_and(|part| part.read(cx).thumbnail(index).is_some())
}

#[gpui::test]
async fn an_image_row_gets_its_thumbnail(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let (_temp, view, cx, _core) = window_on_media(cx, "thumbnail").await;

    view.condition::<WindowEvent>(cx, |view, cx| has_thumbnail(view, 1, cx))
        .await;

    view.read_with(cx, |view, cx| assert!(!has_thumbnail(view, 0, cx)));
}

#[gpui::test]
async fn the_quick_look_shows_the_picture_itself(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    cx.update(nova_files_window::bind_window_keys);
    let (_temp, view, cx, _core) = window_on_media(cx, "preview").await;

    let row = cx.debug_bounds("entry-1").unwrap();
    cx.simulate_click(row.center(), Modifiers::none());
    cx.simulate_keystrokes("space");
    view.condition::<WindowEvent>(cx, |view, cx| {
        previewed(view, cx).is_some_and(|shown| shown.picture)
    })
    .await;
    let preview = view.read_with(cx, |view, cx| previewed(view, cx).unwrap().preview.unwrap());
    match &preview.content {
        PreviewContent::Picture(picture) => {
            assert_eq!((picture.width, picture.height), (SIDE, SIDE));
        }
        other => unreachable!("expected a picture, got {other:?}"),
    }

    cx.run_until_parked();
    assert!(cx.debug_bounds("preview-picture").is_some());

    cx.simulate_keystrokes("space");
    view.read_with(cx, |view, cx| assert!(!previewing(view, cx)));
}

#[gpui::test]
async fn a_smaller_window_gets_a_smaller_picture(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    cx.update(nova_files_window::bind_window_keys);
    let (_temp, view, cx, _core) = window_on_media(cx, "preview-resize").await;
    let side = |view: &Entity<MainWindow>, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, cx| {
            let shown = previewed(view, cx)?;
            match &shown.preview?.content {
                PreviewContent::Picture(picture) if shown.picture => Some(picture.width),
                _ => None,
            }
        })
    };

    let row = cx.debug_bounds("entry-1").unwrap();
    cx.simulate_click(row.center(), Modifiers::none());
    cx.simulate_keystrokes("space");
    view.condition::<WindowEvent>(cx, |view, cx| {
        previewed(view, cx).is_some_and(|shown| shown.picture)
    })
    .await;
    assert_eq!(side(&view, cx), Some(SIDE));

    cx.simulate_resize(gpui::size(gpui::px(200.0), gpui::px(200.0)));
    cx.run_until_parked();
    view.condition::<WindowEvent>(cx, |view, cx| {
        previewed(view, cx)
            .and_then(|shown| shown.preview)
            .is_some_and(|preview| match &preview.content {
                PreviewContent::Picture(picture) => picture.width < SIDE,
                _ => false,
            })
    })
    .await;
}
