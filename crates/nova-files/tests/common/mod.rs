//! What the window tests share: a temp folder tree, a watchdog for waits
//! on the real core, and a double-click.

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use gpui::{Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, VisualTestContext};

/// A folder under the system temp folder, removed when dropped: a folder
/// `Projects` (holding `nova`) and a file `notes.txt`.
pub struct TempFolder(PathBuf);

impl TempFolder {
    /// The tree, in a folder named after `test` so runs don't collide.
    pub fn tree(test: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "nova-files-{test}-test-{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        // A core's state writer can finish after an earlier test's tree is
        // gone and make its state folder again; a later test given the same
        // process id would find it.
        fs::remove_dir_all(&path).ok();
        fs::create_dir_all(path.join("Projects").join("nova")).unwrap();
        fs::write(path.join("notes.txt"), "").unwrap();
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

/// How long a test may wait for the core before it fails.
pub const WAIT: Duration = Duration::from_secs(10);

/// Fails the test process if the test is still running after [`WAIT`].
/// GPUI's waits have no deadline of their own while the core answers from
/// real threads, so a window that never gets there would hang.
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

/// Double-clicks the element drawn with `selector`.
pub fn double_click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    let position = cx.debug_bounds(selector).unwrap().center();
    for click_count in 1..=2 {
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
}
