//! A window and the real core, wired as the program wires them, against a
//! real temp folder tree: the window connects, opens its folder, and goes
//! into a folder, up, or to a place when clicked. A folder that can't be
//! read leaves it where it was. Each entry's Modified, Type and Size fill
//! in once the core has read them. A picker window picks the file
//! double-clicked when it asks for files, and answers cancelled when it
//! is closed.

#![expect(
    clippy::unwrap_used,
    clippy::future_not_send,
    reason = "test setup should fail loudly; GPUI test contexts live on one thread"
)]

use std::cell::RefCell;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use gpui::{
    Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, TestAppContext, VisualTestContext,
};
use nova_files_api::{PickAnswer, PickId, PickKind, PickReply, PickRequest};
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
            process::id(),
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

/// How long a test may wait for the core before it fails.
const WAIT: Duration = Duration::from_secs(10);

/// Fails the test process if the test is still running after [`WAIT`].
/// GPUI's waits have no deadline of their own while the core answers from
/// real threads, so a window that never gets there would hang.
struct Watchdog(Sender<()>);

impl Watchdog {
    fn start() -> Self {
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

/// Opens a window on `folder` with a running core, and waits until the
/// folder is on screen.
async fn open_on<'a>(
    cx: &'a mut TestAppContext,
    folder: &Path,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    let (view, cx) = window_on(cx, folder, folder);
    wait_for_folder(&view, cx, folder).await;
    (view, cx)
}

/// Opens a window on `folder` with a running core, both taking `home` as
/// the home folder; doesn't wait.
fn window_on<'a>(
    cx: &'a mut TestAppContext,
    folder: &Path,
    home: &Path,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    // The core answers from its own threads.
    cx.executor().allow_parking();
    let core_home = nova_files_core::Home {
        folder: home.to_owned(),
        config: home.join(".config"),
    };
    let core =
        nova_files_core::spawn_with(nova_files_core::IDLE_EXIT, Some(core_home), None).unwrap();
    let (start, home) = (folder.to_owned(), home.to_owned());
    cx.add_window_view(|_, cx| MainWindow::with_home(&core.connector, &start, home, cx))
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
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let (view, cx) = open_on(cx, tree.path()).await;

    view.condition::<WindowEvent>(cx, |view, _| view.status() == CoreStatus::Connected)
        .await;
}

#[gpui::test]
fn window_without_a_core_still_opens(cx: &mut TestAppContext) {
    let core = nova_files_core::spawn_with_idle_exit(Duration::ZERO).unwrap();
    core.thread.join().unwrap();

    let window = cx.add_window(|_, cx| MainWindow::new(&core.connector, Path::new("/"), cx));
    let view = window.root(cx).unwrap();

    view.read_with(cx, |view, _| assert_eq!(view.status(), CoreStatus::Gone));
}

#[gpui::test]
async fn shows_the_folder_it_opened_on(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let (view, cx) = open_on(cx, tree.path()).await;

    view.read_with(cx, |view, _| {
        assert_eq!(view.entry_names(), ["Projects", "notes.txt"]);
    });
}

#[gpui::test]
async fn clicking_a_folder_goes_into_it_and_up_comes_back(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
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
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let (view, cx) = open_on(cx, tree.path()).await;

    click(cx, "entry-1");
    cx.run_until_parked();

    view.read_with(cx, |view, _| assert_eq!(view.folder(), Some(tree.path())));
}

#[gpui::test]
async fn unreadable_folder_leaves_the_window_where_it_was(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let (view, cx) = open_on(cx, tree.path()).await;
    let gone = tree.path().join("Projects");
    fs::remove_dir_all(&gone).unwrap();

    let refused = Rc::new(RefCell::new(Vec::new()));
    let _watch = cx.update(|_, cx| {
        let refused = Rc::clone(&refused);
        cx.subscribe(&view, move |_, event: &WindowEvent, _| {
            if let WindowEvent::Refused(path) = event {
                refused.borrow_mut().push(path.clone());
            }
        })
    });

    // The listing still shows Projects; the folder vanished meanwhile.
    click(cx, "entry-0");
    let seen = Rc::clone(&refused);
    view.condition::<WindowEvent>(cx, move |_, _| !seen.borrow().is_empty())
        .await;

    assert_eq!(*refused.borrow(), [gone]);
    view.read_with(cx, |view, _| {
        assert_eq!(view.folder(), Some(tree.path()));
        assert_eq!(view.entry_names(), ["Projects", "notes.txt"]);
    });
}

#[gpui::test]
async fn unreadable_start_folder_falls_back_to_home(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let missing = tree.path().join("missing");
    let (view, cx) = window_on(cx, &missing, tree.path());

    wait_for_folder(&view, cx, tree.path()).await;
}

#[gpui::test]
async fn dots_in_a_path_are_tidied(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let dotted = tree.path().join("Projects").join("..");
    let (view, cx) = window_on(cx, &dotted, tree.path());

    wait_for_folder(&view, cx, tree.path()).await;
}

#[gpui::test]
async fn clicking_a_place_opens_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let home = tree.path().join("Projects");
    let (view, cx) = window_on(cx, tree.path(), &home);
    wait_for_folder(&view, cx, tree.path()).await;
    view.condition::<WindowEvent>(cx, |view, _| !view.places().is_empty())
        .await;

    click(cx, "place-0");

    wait_for_folder(&view, cx, &home).await;
}

#[gpui::test]
async fn columns_fill_in_after_the_names(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let (view, cx) = open_on(cx, tree.path()).await;

    view.condition::<WindowEvent>(cx, |view, _| {
        view.columns(1).is_some_and(|text| !text.size.is_empty())
    })
    .await;

    view.read_with(cx, |view, _| {
        let folder = view.columns(0).unwrap();
        assert_ne!(folder.modified, "");
        assert_eq!(
            (folder.type_name.as_str(), folder.size.as_str()),
            ("Folder", "")
        );
        let file = view.columns(1).unwrap();
        assert_eq!(
            (file.type_name.as_str(), file.size.as_str()),
            ("Text", "0 B")
        );
        assert_eq!(view.columns(2), None);
    });
}

/// A pick request for files to open, or a place to save at, whose answer
/// arrives on the receiver.
fn pick_request(kind: PickKind) -> (Box<PickRequest>, mpsc::Receiver<PickAnswer>) {
    let (sender, answers) = mpsc::channel();
    let request = PickRequest {
        id: PickId(9),
        kind,
        title: "Open".to_owned(),
        accept_label: None,
        filters: Vec::new(),
        filter: None,
        folder: None,
        reply: PickReply::new(move |answer| {
            sender.send(answer).ok();
        }),
    };
    (Box::new(request), answers)
}

const OPEN_ONE: PickKind = PickKind::Open {
    multiple: false,
    folders: false,
};

/// Double-clicks the element drawn with `selector`.
fn double_click(cx: &mut VisualTestContext, selector: &'static str) {
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

#[gpui::test]
async fn a_picker_picks_the_file_double_clicked(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let (view, cx) = open_on(cx, tree.path()).await;
    let (request, answers) = pick_request(OPEN_ONE);
    view.update(cx, |view, _| view.set_pick(request));

    click(cx, "entry-1");
    assert!(answers.try_recv().is_err());

    double_click(cx, "entry-1");
    assert_eq!(
        answers.try_recv(),
        Ok(PickAnswer::Picked {
            paths: vec![tree.path().join("notes.txt")],
            filter: None
        })
    );
}

#[gpui::test]
async fn a_picker_still_goes_into_folders(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let (view, cx) = open_on(cx, tree.path()).await;
    let (request, answers) = pick_request(OPEN_ONE);
    view.update(cx, |view, _| view.set_pick(request));

    click(cx, "entry-0");
    wait_for_folder(&view, cx, &tree.path().join("Projects")).await;

    assert!(answers.try_recv().is_err());
    view.read_with(cx, |view, _| assert_eq!(view.pick_id(), Some(PickId(9))));
}

#[gpui::test]
async fn a_save_picker_leaves_double_clicked_files_alone(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let (view, cx) = open_on(cx, tree.path()).await;
    let (request, answers) = pick_request(PickKind::Save {
        name: "a.txt".to_owned(),
    });
    view.update(cx, |view, _| view.set_pick(request));

    double_click(cx, "entry-1");

    assert!(answers.try_recv().is_err());
    view.read_with(cx, |view, _| assert_eq!(view.pick_id(), Some(PickId(9))));
}

#[gpui::test]
async fn closing_a_picker_answers_cancelled(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let (view, cx) = open_on(cx, tree.path()).await;
    let (request, answers) = pick_request(OPEN_ONE);
    view.update(cx, |view, _| view.set_pick(request));

    // From the app, not a window: the UI closes pickers from its request
    // loop, with no window borrowed.
    let app: &mut TestAppContext = cx;
    app.update(|cx| nova_files_ui::close_picker(PickId(8), cx));
    cx.run_until_parked();
    assert!(answers.try_recv().is_err());

    let app: &mut TestAppContext = cx;
    app.update(|cx| nova_files_ui::close_picker(PickId(9), cx));
    assert_eq!(app.update(|cx| cx.windows().len()), 0);
    assert_eq!(answers.try_recv(), Ok(PickAnswer::Cancelled));
    drop(view);
}
