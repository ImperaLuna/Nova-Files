//! A window and the real core, wired as the program wires them, against a
//! real temp folder tree: the window connects, opens its folder, and goes
//! into a folder, up, or to a place when clicked. A folder that can't be
//! read leaves it where it was.

#![expect(
    clippy::unwrap_used,
    clippy::future_not_send,
    reason = "test setup should fail loudly; GPUI test contexts live on one thread"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
use nova_files_ui::{CoreStatus, MainWindow, WindowEvent};

/// A folder under the system temp folder, removed when dropped.
struct TempFolder(PathBuf);

impl TempFolder {
    /// A tree with a folder `Projects` (holding `nova`) and a file
    /// `notes.txt`.
    fn tree() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "nova-files-window-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        fs::create_dir_all(path.join("Projects").join("nova")).unwrap();
        fs::write(path.join("notes.txt"), "").unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

/// Opens a window on `folder` with a running core, and waits until the
/// folder is on screen.
async fn open_on<'a>(
    cx: &'a mut TestAppContext,
    folder: &Path,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    // The core answers from its own threads.
    cx.executor().allow_parking();
    let core = nova_files_core::spawn().unwrap();
    let start = folder.to_owned();
    let (view, cx) = cx.add_window_view(|_, cx| MainWindow::new(&core.connector, &start, cx));
    wait_for_folder(&view, cx, folder).await;
    (view, cx)
}

async fn wait_for_folder(view: &Entity<MainWindow>, cx: &VisualTestContext, folder: &Path) {
    view.condition::<WindowEvent>(cx, |view, _| view.folder() == Some(folder))
        .await;
}

/// Clicks the element drawn with `selector`.
fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    let bounds = cx.debug_bounds(selector).unwrap();
    cx.simulate_click(bounds.center(), Modifiers::none());
}

#[gpui::test]
async fn window_connects_to_the_core(cx: &mut TestAppContext) {
    let tree = TempFolder::tree();
    let (view, cx) = open_on(cx, tree.path()).await;

    view.condition::<WindowEvent>(cx, |view, _| view.status() == CoreStatus::Connected)
        .await;
}

#[gpui::test]
fn window_without_a_core_still_opens(cx: &mut TestAppContext) {
    let core = nova_files_core::spawn_with_idle_exit(std::time::Duration::ZERO).unwrap();
    core.thread.join().unwrap();

    let window = cx.add_window(|_, cx| MainWindow::new(&core.connector, Path::new("/"), cx));
    let view = window.root(cx).unwrap();

    view.read_with(cx, |view, _| assert_eq!(view.status(), CoreStatus::Gone));
}

#[gpui::test]
async fn shows_the_folder_it_opened_on(cx: &mut TestAppContext) {
    let tree = TempFolder::tree();
    let (view, cx) = open_on(cx, tree.path()).await;

    view.read_with(cx, |view, _| {
        assert_eq!(view.entry_names(), ["Projects", "notes.txt"]);
    });
}

#[gpui::test]
async fn clicking_a_folder_goes_into_it_and_up_comes_back(cx: &mut TestAppContext) {
    let tree = TempFolder::tree();
    let (view, cx) = open_on(cx, tree.path()).await;

    click(cx, "entry-0");
    let projects = tree.path().join("Projects");
    wait_for_folder(&view, cx, &projects).await;
    view.read_with(cx, |view, _| assert_eq!(view.entry_names(), ["nova"]));

    click(cx, "up");
    wait_for_folder(&view, cx, tree.path()).await;
}

#[gpui::test]
async fn clicking_a_file_stays_put(cx: &mut TestAppContext) {
    let tree = TempFolder::tree();
    let (view, cx) = open_on(cx, tree.path()).await;

    click(cx, "entry-1");
    cx.run_until_parked();

    view.read_with(cx, |view, _| assert_eq!(view.folder(), Some(tree.path())));
}

#[gpui::test]
async fn unreadable_folder_leaves_the_window_where_it_was(cx: &mut TestAppContext) {
    let tree = TempFolder::tree();
    let (view, cx) = open_on(cx, tree.path()).await;
    let gone = tree.path().join("Projects");
    fs::remove_dir_all(&gone).unwrap();

    // The listing still shows Projects; the folder vanished meanwhile.
    click(cx, "entry-0");
    view.read_with(cx, |view, _| assert!(view.is_loading()));
    view.condition::<WindowEvent>(cx, |view, _| !view.is_loading())
        .await;

    view.read_with(cx, |view, _| {
        assert_eq!(view.folder(), Some(tree.path()));
        assert_eq!(view.entry_names(), ["Projects", "notes.txt"]);
    });
}

#[gpui::test]
async fn unreadable_start_folder_falls_back_to_home(cx: &mut TestAppContext) {
    let tree = TempFolder::tree();
    cx.executor().allow_parking();
    let core = nova_files_core::spawn().unwrap();
    let missing = tree.path().join("missing");
    let (view, cx) = cx.add_window_view(|_, cx| MainWindow::new(&core.connector, &missing, cx));

    view.condition::<WindowEvent>(cx, |view, _| view.folder().is_some())
        .await;

    view.read_with(cx, |view, _| {
        assert_eq!(view.folder(), Some(nova_files_ui::home_folder().as_path()));
    });
}

#[gpui::test]
async fn dots_in_a_path_are_tidied(cx: &mut TestAppContext) {
    let tree = TempFolder::tree();
    let dotted = tree.path().join("Projects").join("..");
    cx.executor().allow_parking();
    let core = nova_files_core::spawn().unwrap();
    let (view, cx) = cx.add_window_view(|_, cx| MainWindow::new(&core.connector, &dotted, cx));

    wait_for_folder(&view, cx, tree.path()).await;
}

#[gpui::test]
async fn clicking_a_place_opens_it(cx: &mut TestAppContext) {
    let tree = TempFolder::tree();
    let (view, cx) = open_on(cx, tree.path()).await;
    view.condition::<WindowEvent>(cx, |view, _| view.status() == CoreStatus::Connected)
        .await;
    cx.run_until_parked();
    // Places come from the real home folder; a build sandbox may have none.
    if !nova_files_ui::home_folder().is_dir() {
        return;
    }
    view.condition::<WindowEvent>(cx, |view, _| !view.places().is_empty())
        .await;
    let first = view.read_with(cx, |view, _| view.places()[0].clone());

    click(cx, "place-0");

    wait_for_folder(&view, cx, &first.path).await;
}
