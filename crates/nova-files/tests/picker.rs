//! A picker window and the real core against a real temp folder tree,
//! driven through its pick bar: clicks copy a file's name into the name
//! box, Enter and the Open or Save button do what the box says, typed
//! paths go there, folders can be picked, a save onto a file that is there
//! asks Replace or Keep both, and Cancel or Escape answers cancelled.

#![expect(
    clippy::unwrap_used,
    clippy::future_not_send,
    reason = "test setup should fail loudly; GPUI test contexts live on one thread"
)]

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write as _};
use std::os::unix::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use gpui::{
    Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, TestAppContext, VisualTestContext,
};
use nova_files_api::{EntryId, PickAnswer, PickId, PickKind, PickReply, PickRequest};
use nova_files_ui::{MainWindow, WindowEvent};

/// A folder under the system temp folder, removed when dropped: a folder
/// `Projects` (holding `nova`) and the files `notes.txt` and
/// `notes (2).txt`.
struct TempFolder(PathBuf);

impl TempFolder {
    fn tree() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "nova-files-picker-test-{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        fs::create_dir_all(path.join("Projects").join("nova")).unwrap();
        fs::write(path.join("notes.txt"), "").unwrap();
        fs::write(path.join("notes (2).txt"), "").unwrap();
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
struct Watchdog(Sender<()>);

impl Watchdog {
    fn start() -> Self {
        let (done, finished) = mpsc::channel();
        thread::spawn(move || {
            if finished.recv_timeout(WAIT) == Err(RecvTimeoutError::Timeout) {
                writeln!(io::stderr(), "the picker never got there within {WAIT:?}").ok();
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

/// A picker for `kind` on `folder` of `tree`, with the tree as home, once
/// the folder is on screen; and where its answer arrives.
async fn picker<'a>(
    cx: &'a mut TestAppContext,
    tree: &TempFolder,
    folder: &Path,
    kind: PickKind,
) -> (
    Entity<MainWindow>,
    &'a mut VisualTestContext,
    Receiver<PickAnswer>,
) {
    cx.update(nova_files_ui::bind_text_keys);
    // The core answers from its own threads.
    cx.executor().allow_parking();
    let home = tree.path().to_owned();
    let core_home = nova_files_core::Home {
        folder: home.clone(),
        config: home.join(".config"),
    };
    let core =
        nova_files_core::spawn_with(nova_files_core::IDLE_EXIT, Some(core_home), None).unwrap();
    let start = folder.to_owned();
    let (view, cx) =
        cx.add_window_view(|_, cx| MainWindow::with_home(&core.connector, &start, home, cx));
    wait_for_folder(&view, cx, folder).await;
    let (sender, answers) = mpsc::channel();
    let request = Box::new(PickRequest {
        id: PickId(9),
        kind,
        title: String::new(),
        accept_label: None,
        filters: Vec::new(),
        filter: None,
        folder: None,
        reply: PickReply::new(move |answer| {
            sender.send(answer).ok();
        }),
    });
    view.update_in(cx, |view, window, cx| view.set_pick(request, window, cx));
    cx.run_until_parked();
    (view, cx, answers)
}

async fn wait_for_folder(view: &Entity<MainWindow>, cx: &VisualTestContext, folder: &Path) {
    view.condition::<WindowEvent>(cx, |view, _| view.folder() == Some(folder))
        .await;
}

/// Waits until the name box holds `text`.
async fn wait_for_name(view: &Entity<MainWindow>, cx: &VisualTestContext, text: &str) {
    view.condition::<WindowEvent>(cx, |view, cx| view.pick_name(cx).as_deref() == Some(text))
        .await;
}

fn name(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Option<String> {
    view.read_with(cx, MainWindow::pick_name)
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    let bounds = cx.debug_bounds(selector).unwrap();
    cx.simulate_click(bounds.center(), Modifiers::none());
}

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

fn picked(path: PathBuf) -> PickAnswer {
    PickAnswer::Picked {
        paths: vec![path],
        filter: None,
    }
}

/// Whether the picker's window has closed.
fn closed(cx: &mut VisualTestContext) -> bool {
    let app: &mut TestAppContext = cx;
    app.update(|cx| cx.windows().is_empty())
}

const FILES: PickKind = PickKind::Open {
    multiple: false,
    folders: false,
};
const FOLDERS: PickKind = PickKind::Open {
    multiple: false,
    folders: true,
};

fn save(name: impl Into<OsString>) -> PickKind {
    PickKind::Save { name: name.into() }
}

// Rows on screen: 0 `Projects`, 1 `notes (2).txt`, 2 `notes.txt`.

#[gpui::test]
async fn a_click_puts_the_name_in_the_box_and_open_picks_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), FILES).await;
    assert_eq!(name(&view, cx).as_deref(), Some(""));

    click(cx, "entry-2");
    assert_eq!(name(&view, cx).as_deref(), Some("notes.txt"));
    let shown = view.read_with(cx, |view, _| view.listing_id()).unwrap();
    let selection = view.read_with(cx, |view, _| view.pick_selection());
    assert_eq!(selection, Some((shown, EntryId(2))));
    assert!(answers.try_recv().is_err());

    click(cx, "pick-accept");
    assert_eq!(
        answers.try_recv().ok(),
        Some(picked(tree.path().join("notes.txt")))
    );
    assert!(closed(cx));
}

#[gpui::test]
async fn enter_on_a_typed_name_picks_that_file(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let (_view, cx, answers) = picker(cx, &tree, tree.path(), FILES).await;
    cx.simulate_input("missing.txt");
    cx.simulate_keystrokes("enter");
    assert!(answers.try_recv().is_err());

    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("notes (2).txt");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        answers.try_recv(),
        Ok(picked(tree.path().join("notes (2).txt")))
    );
}

#[gpui::test]
async fn a_typed_folder_name_goes_into_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), FILES).await;
    cx.simulate_input("Projects");
    cx.simulate_keystrokes("enter");
    wait_for_folder(&view, cx, &tree.path().join("Projects")).await;
    assert!(answers.try_recv().is_err());
}

#[gpui::test]
async fn typed_paths_go_there(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let projects = tree.path().join("Projects");
    let (view, cx, answers) = picker(cx, &tree, &projects, FILES).await;

    // `~/` is home: here, the tree.
    cx.simulate_input("~/");
    cx.simulate_keystrokes("enter");
    wait_for_folder(&view, cx, tree.path()).await;
    assert_eq!(name(&view, cx).as_deref(), Some(""));

    // A full path to a folder goes there.
    let nova = projects.join("nova");
    cx.simulate_input(&nova.to_string_lossy());
    cx.simulate_keystrokes("enter");
    wait_for_folder(&view, cx, &nova).await;
    assert_eq!(name(&view, cx).as_deref(), Some(""));
    assert!(answers.try_recv().is_err());
}

#[gpui::test]
async fn a_typed_path_to_a_file_goes_to_its_folder_with_its_name(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let projects = tree.path().join("Projects");
    let (view, cx, answers) = picker(cx, &tree, &projects, FILES).await;

    cx.simulate_input("~/notes.txt");
    cx.simulate_keystrokes("enter");
    wait_for_folder(&view, cx, tree.path()).await;
    wait_for_name(&view, cx, "notes.txt").await;
    assert!(answers.try_recv().is_err());

    cx.simulate_keystrokes("enter");
    assert_eq!(
        answers.try_recv().ok(),
        Some(picked(tree.path().join("notes.txt")))
    );
}

#[gpui::test]
async fn a_save_selects_the_name_before_its_extension(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let (_view, cx, answers) = picker(cx, &tree, tree.path(), save("report.pdf")).await;
    // Typing replaces just the selected part.
    cx.simulate_input("plan");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        answers.try_recv().ok(),
        Some(picked(tree.path().join("plan.pdf")))
    );
    assert!(closed(cx));
}

#[gpui::test]
async fn a_save_keeps_a_suggested_name_that_is_not_utf8(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let suggested = OsString::from_vec(b"caf\xe9.txt".to_vec());
    let (_view, cx, answers) = picker(cx, &tree, tree.path(), save(suggested.clone())).await;
    click(cx, "pick-accept");
    assert_eq!(
        answers.try_recv().ok(),
        Some(picked(tree.path().join(suggested)))
    );
}

#[gpui::test]
async fn a_save_onto_a_file_asks_and_enter_keeps_both(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), save("notes.txt")).await;
    cx.simulate_keystrokes("enter");
    let asked = view.read_with(cx, |view, _| view.pick_conflict().map(ToOwned::to_owned));
    // `notes (2).txt` is taken too.
    assert_eq!(asked, Some(OsString::from("notes (3).txt")));
    assert!(answers.try_recv().is_err());

    cx.simulate_keystrokes("enter");
    assert_eq!(
        answers.try_recv(),
        Ok(picked(tree.path().join("notes (3).txt")))
    );
}

#[gpui::test]
async fn replace_saves_over_the_file(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let (_view, cx, answers) = picker(cx, &tree, tree.path(), save("notes.txt")).await;
    click(cx, "pick-accept");
    click(cx, "pick-replace");
    assert_eq!(
        answers.try_recv().ok(),
        Some(picked(tree.path().join("notes.txt")))
    );
}

#[gpui::test]
async fn keep_both_saves_beside_the_file(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let (_view, cx, answers) = picker(cx, &tree, tree.path(), save("notes.txt")).await;
    click(cx, "pick-accept");
    click(cx, "pick-keep-both");
    assert_eq!(
        answers.try_recv(),
        Ok(picked(tree.path().join("notes (3).txt")))
    );
}

#[gpui::test]
async fn typing_after_the_question_takes_it_back(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), save("notes.txt")).await;
    cx.simulate_keystrokes("enter");
    cx.simulate_keystrokes("end backspace backspace backspace");
    cx.simulate_input("md");
    let asked = view.read_with(cx, |view, _| view.pick_conflict().map(ToOwned::to_owned));
    assert_eq!(asked, None);
    cx.simulate_keystrokes("enter");
    assert_eq!(
        answers.try_recv().ok(),
        Some(picked(tree.path().join("notes.md")))
    );
}

#[gpui::test]
async fn clicking_a_file_in_a_save_copies_its_name(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), save("a.txt")).await;
    double_click(cx, "entry-1");
    assert_eq!(name(&view, cx).as_deref(), Some("notes (2).txt"));
    assert!(answers.try_recv().is_err());
}

#[gpui::test]
async fn a_folder_picker_picks_the_folder_on_screen(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let (_view, cx, answers) = picker(cx, &tree, tree.path(), FOLDERS).await;
    // A file is no folder to pick.
    click(cx, "entry-2");
    click(cx, "pick-accept");
    assert_eq!(
        answers.try_recv().ok(),
        Some(picked(tree.path().to_owned()))
    );
}

#[gpui::test]
async fn a_folder_picker_picks_the_folder_clicked(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), FOLDERS).await;
    click(cx, "entry-0");
    // A click selects; it doesn't go in.
    cx.run_until_parked();
    assert!(!view.read_with(cx, |view, _| view.waiting()));
    let selection = view.read_with(cx, |view, _| view.pick_selection().map(|(_, id)| id));
    assert_eq!(selection, Some(EntryId(0)));
    click(cx, "pick-accept");
    assert_eq!(
        answers.try_recv().ok(),
        Some(picked(tree.path().join("Projects")))
    );
}

#[gpui::test]
async fn a_folder_picker_goes_into_a_folder_double_clicked(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), FOLDERS).await;
    double_click(cx, "entry-0");
    wait_for_folder(&view, cx, &tree.path().join("Projects")).await;
    // The folder it left took its selection along.
    assert_eq!(view.read_with(cx, |view, _| view.pick_selection()), None);
    click(cx, "pick-accept");
    assert_eq!(
        answers.try_recv().ok(),
        Some(picked(tree.path().join("Projects")))
    );
}

#[gpui::test]
async fn cancel_answers_cancelled(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let (_view, cx, answers) = picker(cx, &tree, tree.path(), FILES).await;
    click(cx, "pick-cancel");
    assert_eq!(answers.try_recv(), Ok(PickAnswer::Cancelled));
    assert!(closed(cx));
}

#[gpui::test]
async fn escape_answers_cancelled(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let (_view, cx, answers) = picker(cx, &tree, tree.path(), save("a.txt")).await;
    cx.simulate_keystrokes("escape");
    assert_eq!(answers.try_recv(), Ok(PickAnswer::Cancelled));
    assert!(closed(cx));
}

#[gpui::test]
async fn nothing_is_picked_while_a_folder_loads(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), FOLDERS).await;
    view.update(cx, |view, _| view.open(&tree.path().join("Projects")));
    view.update_in(cx, MainWindow::accept);
    assert!(answers.try_recv().is_err());
}
