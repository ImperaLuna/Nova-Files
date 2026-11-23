use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

pub mod picking;

use gpui::{
    Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, VisualTestContext,
};
use nova_files_test_folder::TestFolder;

pub struct TempFolder {
    tree: PathBuf,
    _folder: TestFolder,
}

impl TempFolder {
    pub fn tree(test: &str) -> Self {
        let folder = TestFolder::new(&format!("{test}-test"));
        let tree = folder.join("tree");
        fs::create_dir_all(tree.join("Projects").join("nova")).unwrap();
        fs::write(tree.join("notes.txt"), "").unwrap();
        Self {
            tree,
            _folder: folder,
        }
    }

    pub fn path(&self) -> &Path {
        &self.tree
    }
}

pub const WAIT: Duration = Duration::from_secs(10);

pub struct Watchdog(Sender<()>);

impl Watchdog {
    pub fn start() -> Self {
        let (done, finished) = mpsc::channel();
        thread::spawn(move || {
            if finished.recv_timeout(WAIT) == Err(RecvTimeoutError::Timeout) {
                writeln!(io::stderr(), "the window never got there within {WAIT:?}").ok();
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

pub fn click_with(cx: &mut VisualTestContext, selector: &'static str, modifiers: Modifiers) {
    cx.run_until_parked();
    let bounds = cx.debug_bounds(selector).unwrap();
    cx.simulate_click(bounds.center(), modifiers);
}

pub fn double_click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    let position = cx.debug_bounds(selector).unwrap().center();
    for click_count in 1..=2 {
        counted_click(cx, position, click_count);
    }
}

pub fn counted_click(cx: &mut VisualTestContext, position: Point<Pixels>, click_count: usize) {
    cx.simulate_event(MouseDownEvent {
        position,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count,
    });
}
