//! A window on a real core and the real helper: an image row in the list
//! gets its thumbnail once the list has drawn it.

#![expect(
    clippy::future_not_send,
    reason = "GPUI test contexts live on one thread"
)]

use std::fs;
use std::path::PathBuf;

use gpui::TestAppContext;
use image::{ImageFormat, RgbImage};
use nova_files_core::{IDLE_EXIT, Thumbnails};
use nova_files_ui::{MainWindow, WindowEvent};

const HELPER: &str = env!("CARGO_BIN_EXE_nova-files-thumbnailer");
const SIDE: u32 = 300;

/// A folder under the system temp folder, removed when dropped.
struct TempFolder(PathBuf);

impl Drop for TempFolder {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

#[gpui::test]
async fn an_image_row_gets_its_thumbnail(cx: &mut TestAppContext) {
    let temp = TempFolder(
        std::env::temp_dir().join(format!("nova-files-thumbs-window-{}", std::process::id())),
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
    let core = nova_files_core::spawn_with(IDLE_EXIT, None, Some(setup)).unwrap();
    let (start, home) = (folder.clone(), temp.0.clone());
    let (view, cx) =
        cx.add_window_view(|_, cx| MainWindow::with_home(&core.connector, &start, home, cx));

    view.condition::<WindowEvent>(cx, |view, _| view.folder() == Some(folder.as_path()))
        .await;
    cx.run_until_parked();
    view.condition::<WindowEvent>(cx, |view, _| view.thumbnail(1).is_some())
        .await;

    view.read_with(cx, |view, _| assert!(view.thumbnail(0).is_none()));
}
