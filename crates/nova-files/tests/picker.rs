//! A picker window and the real core against a real temp folder tree,
//! driven through its pick bar: clicks copy a file's name into the name
//! box, Enter and the Open or Save button do what the box says, typed
//! paths go there, folders can be picked, a save onto a file that is there
//! asks Replace or Keep both, Cancel or Escape answers cancelled, and the
//! app's file types dim the files they leave out.

#![expect(
    clippy::unwrap_used,
    clippy::future_not_send,
    reason = "test setup should fail loudly; GPUI test contexts live on one thread"
)]

mod common;
mod pick_filter;
mod pick_names;
mod pick_several;
mod pick_state;

use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use gpui::{
    Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, PlatformInput, TestAppContext,
    VisualTestContext,
};
use nova_files_api::{EntryId, PickAnswer, PickId, PickKind, PickReply, PickRequest};
use nova_files_ui::{MainWindow, WindowEvent};

use common::{TempFolder, Watchdog, double_click};

/// The shared tree plus `notes (2).txt`.
fn picker_tree() -> TempFolder {
    let tree = TempFolder::tree("picker");
    fs::write(tree.path().join("notes (2).txt"), "").unwrap();
    tree
}

/// The answer, once the picker has given one. A save answers only after a
/// fresh listing of its folder, so it takes a trip to the core.
async fn answer(
    view: &Entity<MainWindow>,
    cx: &VisualTestContext,
    answers: &Receiver<PickAnswer>,
) -> Option<PickAnswer> {
    view.condition::<WindowEvent>(cx, |view, _| view.pick_id().is_none())
        .await;
    answers.try_recv().ok()
}

/// Waits until the bar asks Replace or Keep both, and returns the
/// numbered name it offers.
async fn asked(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Option<OsString> {
    view.condition::<WindowEvent>(cx, |view, _| view.pick_conflict().is_some())
        .await;
    view.read_with(cx, |view, _| view.pick_conflict().map(ToOwned::to_owned))
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
    picker_with(cx, tree, folder, kind, |_| {}).await
}

/// [`picker`], with `ask` filling in more of the request.
async fn picker_with<'a>(
    cx: &'a mut TestAppContext,
    tree: &TempFolder,
    folder: &Path,
    kind: PickKind,
    ask: impl FnOnce(&mut PickRequest),
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
        state: home.join(".local/state"),
    };
    let core = nova_files_core::spawn_with(nova_files_core::IDLE_EXIT, Some(core_home), None, None)
        .unwrap();
    let start = folder.to_owned();
    let (view, cx) =
        cx.add_window_view(|_, cx| MainWindow::with_home(&core.connector, &start, home, cx));
    wait_for_folder(&view, cx, folder).await;
    let (sender, answers) = mpsc::channel();
    let mut request = Box::new(PickRequest {
        id: PickId(9),
        kind,
        title: String::new(),
        accept_label: None,
        filters: Vec::new(),
        filter: None,
        folder: None,
        parent: None,
        types: None,
        reply: PickReply::new(move |answer| {
            sender.send(answer).ok();
        }),
    });
    ask(&mut request);
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
    let tree = picker_tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), FILES).await;
    assert_eq!(name(&view, cx).as_deref(), Some(""));

    click(cx, "entry-2");
    assert_eq!(name(&view, cx).as_deref(), Some("notes.txt"));
    let shown = view.read_with(cx, |view, _| view.listing_id()).unwrap();
    let selection = view.read_with(cx, |view, _| view.pick_selection());
    assert_eq!(selection, Some((shown, vec![EntryId(2)])));
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
    let tree = picker_tree();
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
    let tree = picker_tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), FILES).await;
    cx.simulate_input("Projects");
    cx.simulate_keystrokes("enter");
    wait_for_folder(&view, cx, &tree.path().join("Projects")).await;
    assert!(answers.try_recv().is_err());
}

#[gpui::test]
async fn the_window_keys_work_from_the_name_box(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    cx.update(nova_files_ui::bind_window_keys);
    let projects = tree.path().join("Projects");
    let (view, cx, _answers) = picker(cx, &tree, &projects, FILES).await;
    cx.simulate_keystrokes("alt-up");
    wait_for_folder(&view, cx, tree.path()).await;
}

#[gpui::test]
async fn typing_goes_to_the_name_box_after_focus_is_lost(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (view, cx, _answers) = picker(cx, &tree, tree.path(), FILES).await;
    cx.update(|window, _| window.blur());
    cx.run_until_parked();
    cx.simulate_input("plan");
    assert_eq!(name(&view, cx).as_deref(), Some("plan"));
}

#[gpui::test]
async fn typed_paths_go_there(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
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
    let tree = picker_tree();
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
    let tree = picker_tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), save("report.pdf")).await;
    // Typing replaces just the selected part.
    cx.simulate_input("plan");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        answer(&view, cx, &answers).await,
        Some(picked(tree.path().join("plan.pdf")))
    );
    cx.run_until_parked();
    assert!(closed(cx));
}

#[gpui::test]
async fn a_save_onto_a_file_asks_and_enter_keeps_both(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), save("notes.txt")).await;
    cx.simulate_keystrokes("enter");
    // `notes (2).txt` is taken too.
    assert_eq!(
        asked(&view, cx).await,
        Some(OsString::from("notes (3).txt"))
    );
    assert!(answers.try_recv().is_err());

    cx.simulate_keystrokes("enter");
    assert_eq!(
        answer(&view, cx, &answers).await,
        Some(picked(tree.path().join("notes (3).txt")))
    );
}

#[gpui::test]
async fn replace_saves_over_the_file(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), save("notes.txt")).await;
    click(cx, "pick-accept");
    asked(&view, cx).await;
    click(cx, "pick-replace");
    assert_eq!(
        answers.try_recv().ok(),
        Some(picked(tree.path().join("notes.txt")))
    );
}

#[gpui::test]
async fn keep_both_saves_beside_the_file(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), save("notes.txt")).await;
    click(cx, "pick-accept");
    asked(&view, cx).await;
    click(cx, "pick-keep-both");
    assert_eq!(
        answer(&view, cx, &answers).await,
        Some(picked(tree.path().join("notes (3).txt")))
    );
}

#[gpui::test]
async fn typing_after_the_question_takes_it_back(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), save("notes.txt")).await;
    cx.simulate_keystrokes("enter");
    asked(&view, cx).await;
    cx.simulate_keystrokes("end backspace backspace backspace");
    cx.simulate_input("md");
    let question = view.read_with(cx, |view, _| view.pick_conflict().map(ToOwned::to_owned));
    assert_eq!(question, None);
    cx.simulate_keystrokes("enter");
    assert_eq!(
        answer(&view, cx, &answers).await,
        Some(picked(tree.path().join("notes.md")))
    );
}

#[gpui::test]
async fn clicking_a_file_in_a_save_copies_its_name(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), save("a.txt")).await;
    double_click(cx, "entry-1");
    assert_eq!(name(&view, cx).as_deref(), Some("notes (2).txt"));
    assert!(answers.try_recv().is_err());
}

#[gpui::test]
async fn a_folder_picker_picks_the_folder_on_screen(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
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
    let tree = picker_tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), FOLDERS).await;
    click(cx, "entry-0");
    // A click selects; it doesn't go in.
    cx.run_until_parked();
    assert!(!view.read_with(cx, |view, _| view.waiting()));
    let selection = view.read_with(cx, |view, _| view.pick_selection().map(|(_, ids)| ids));
    assert_eq!(selection, Some(vec![EntryId(0)]));
    click(cx, "pick-accept");
    assert_eq!(
        answers.try_recv().ok(),
        Some(picked(tree.path().join("Projects")))
    );
}

#[gpui::test]
async fn a_folder_picker_goes_into_a_folder_double_clicked(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), FOLDERS).await;
    double_click(cx, "entry-0");
    view.condition::<WindowEvent>(cx, |view, _| !view.waiting())
        .await;
    let folder = view.read_with(cx, |view, _| view.folder().map(Path::to_path_buf));
    assert_eq!(folder, Some(tree.path().join("Projects")));
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
    let tree = picker_tree();
    let (_view, cx, answers) = picker(cx, &tree, tree.path(), FILES).await;
    click(cx, "pick-cancel");
    assert_eq!(answers.try_recv(), Ok(PickAnswer::Cancelled));
    assert!(closed(cx));
}

#[gpui::test]
async fn escape_answers_cancelled(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (_view, cx, answers) = picker(cx, &tree, tree.path(), save("a.txt")).await;
    cx.simulate_keystrokes("escape");
    assert_eq!(answers.try_recv(), Ok(PickAnswer::Cancelled));
    assert!(closed(cx));
}

#[gpui::test]
async fn nothing_is_picked_while_a_folder_loads(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), FOLDERS).await;
    view.update(cx, |view, _| view.open(&tree.path().join("Projects")));
    view.update_in(cx, MainWindow::accept);
    assert!(answers.try_recv().is_err());
}

#[gpui::test]
async fn a_save_into_a_folder_that_went_away_waits(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let projects = tree.path().join("Projects");
    let (view, cx, answers) = picker(cx, &tree, &projects, save("plan.txt")).await;
    fs::remove_dir_all(&projects).unwrap();
    cx.simulate_keystrokes("enter");
    view.condition::<WindowEvent>(cx, |view, _| !view.waiting())
        .await;

    // Back again and on screen, the folder doesn't take the old save.
    fs::create_dir_all(&projects).unwrap();
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("~/Projects/");
    cx.simulate_keystrokes("enter");
    wait_for_name(&view, cx, "").await;
    cx.run_until_parked();
    assert!(answers.try_recv().is_err());
}

#[gpui::test]
async fn escape_takes_back_the_question_before_it_cancels(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), save("notes.txt")).await;
    cx.simulate_keystrokes("enter");
    asked(&view, cx).await;
    cx.simulate_keystrokes("escape");
    let question = view.read_with(cx, |view, _| view.pick_conflict().map(ToOwned::to_owned));
    assert_eq!(question, None);
    assert!(answers.try_recv().is_err());
    assert!(!closed(cx));

    cx.simulate_keystrokes("escape");
    assert_eq!(answers.try_recv(), Ok(PickAnswer::Cancelled));
}

#[gpui::test]
async fn a_file_made_after_the_listing_still_asks(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), save("late.txt")).await;
    // Another program saves under the same name while the dialog is open.
    fs::write(tree.path().join("late.txt"), "").unwrap();
    cx.simulate_keystrokes("enter");
    assert_eq!(asked(&view, cx).await, Some(OsString::from("late (2).txt")));
    assert!(answers.try_recv().is_err());
}

#[gpui::test]
async fn keep_both_asks_again_when_its_name_was_taken_meanwhile(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), save("notes.txt")).await;
    cx.simulate_keystrokes("enter");
    assert_eq!(
        asked(&view, cx).await,
        Some(OsString::from("notes (3).txt"))
    );
    fs::write(tree.path().join("notes (3).txt"), "").unwrap();
    cx.simulate_keystrokes("enter");
    assert_eq!(
        asked(&view, cx).await,
        Some(OsString::from("notes (4).txt"))
    );
    assert!(answers.try_recv().is_err());
}

#[gpui::test]
async fn a_folder_name_typed_goes_in_and_empties_the_box(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), FOLDERS).await;
    cx.simulate_input("Projects");
    cx.simulate_keystrokes("enter");
    wait_for_folder(&view, cx, &tree.path().join("Projects")).await;
    assert_eq!(name(&view, cx).as_deref(), Some(""));

    click(cx, "pick-accept");
    assert_eq!(
        answers.try_recv().ok(),
        Some(picked(tree.path().join("Projects")))
    );
}

#[gpui::test]
async fn a_clicked_name_means_that_file_even_with_outer_spaces(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    fs::write(tree.path().join(" notes.txt"), "").unwrap();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), FILES).await;
    let row = view
        .read_with(cx, |view, _| view.entry_names())
        .iter()
        .position(|name| name == " notes.txt")
        .unwrap();
    let selector: &'static str = format!("entry-{row}").leak();
    click(cx, selector);
    click(cx, "pick-accept");
    assert_eq!(
        answers.try_recv().ok(),
        Some(picked(tree.path().join(" notes.txt")))
    );
}

#[gpui::test]
async fn going_elsewhere_before_a_save_is_checked_drops_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let projects = tree.path().join("Projects");
    let (view, cx, answers) = picker(cx, &tree, tree.path(), save("plan.txt")).await;
    // Save, then go into a folder before the fresh listing is back.
    view.update_in(cx, |view, window, cx| {
        view.accept(window, cx);
        view.open(&projects);
    });
    wait_for_folder(&view, cx, &projects).await;

    // Back again, the folder doesn't take the old save.
    view.update(cx, |view, _| view.go_up());
    wait_for_folder(&view, cx, tree.path()).await;
    cx.run_until_parked();
    assert!(answers.try_recv().is_err());
    assert!(!closed(cx));
}

#[gpui::test]
async fn dots_typed_go_to_folders(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let projects = tree.path().join("Projects");
    let (view, cx, answers) = picker(cx, &tree, &projects, save("plan.txt")).await;
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("..");
    cx.simulate_keystrokes("enter");
    wait_for_folder(&view, cx, tree.path()).await;
    cx.simulate_input(".");
    cx.simulate_keystrokes("enter");
    wait_for_name(&view, cx, "").await;
    cx.run_until_parked();
    assert!(answers.try_recv().is_err());
    assert!(!closed(cx));
}

#[gpui::test]
async fn clicking_a_file_before_a_save_is_checked_drops_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), save("plan.txt")).await;
    let position = cx.debug_bounds("entry-1").unwrap().center();
    // Clicked before the fresh listing is back.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| view.accept(window, cx));
        let down = MouseDownEvent {
            position,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        };
        let up = MouseUpEvent {
            position,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
        };
        window.dispatch_event(PlatformInput::MouseDown(down), cx);
        window.dispatch_event(PlatformInput::MouseUp(up), cx);
    });
    view.condition::<WindowEvent>(cx, |view, _| !view.waiting())
        .await;
    cx.run_until_parked();
    assert!(answers.try_recv().is_err());
    assert!(!closed(cx));
}

#[gpui::test]
async fn the_question_selects_the_file_in_the_way(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (view, cx, _answers) = picker(cx, &tree, tree.path(), save("notes.txt")).await;
    cx.simulate_keystrokes("enter");
    asked(&view, cx).await;
    assert_selects(&view, cx, "notes.txt");
}

/// Only the row named `selected` shows as the picker's selection.
fn assert_selects(view: &Entity<MainWindow>, cx: &VisualTestContext, selected: &str) {
    view.read_with(cx, |view, _| {
        let listing = view.listing_id().unwrap();
        for (index, name) in view.entry_names().iter().enumerate() {
            let id = EntryId(u32::try_from(index).unwrap());
            assert_eq!(view.pick_selects((listing, id)), name == selected, "{name}");
        }
    });
}

#[gpui::test]
async fn editing_the_name_before_a_save_is_checked_drops_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), save("plan.txt")).await;
    view.update_in(cx, MainWindow::accept);
    // Typed before the fresh listing is back.
    cx.simulate_input("x");
    view.condition::<WindowEvent>(cx, |view, _| !view.waiting())
        .await;
    cx.run_until_parked();
    assert!(answers.try_recv().is_err());
    assert!(!closed(cx));
}

#[gpui::test]
async fn editing_the_name_before_a_typed_folder_opens_keeps_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let projects = tree.path().join("Projects");
    let (view, cx, _answers) = picker(cx, &tree, tree.path(), save("plan.txt")).await;
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("Projects/");
    view.update_in(cx, MainWindow::accept);
    // Typed before the folder is on screen.
    cx.simulate_input("x");
    wait_for_folder(&view, cx, &projects).await;
    cx.run_until_parked();
    assert_eq!(name(&view, cx).as_deref(), Some("Projects/x"));
}

#[gpui::test]
async fn a_name_that_differs_only_in_case_asks_first(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), save("NOTES.txt")).await;
    cx.simulate_keystrokes("enter");
    // `notes (2).txt` is taken too, whatever its case.
    assert_eq!(
        asked(&view, cx).await,
        Some(OsString::from("NOTES (3).txt"))
    );
    assert!(answers.try_recv().is_err());
    // The file in the way shows as selected.
    assert_selects(&view, cx, "notes.txt");
}

#[gpui::test]
async fn going_elsewhere_before_a_typed_folder_opens_drops_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let projects = tree.path().join("Projects");
    let nova = projects.join("nova");
    let (view, cx, _answers) = picker(cx, &tree, tree.path(), save("plan.txt")).await;
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("Projects/");
    view.update_in(cx, |view, window, cx| {
        view.accept(window, cx);
        view.open(&nova);
    });
    wait_for_folder(&view, cx, &nova).await;
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("mine.txt");

    // Visiting the folder later leaves the new name alone.
    view.update(cx, |view, _| view.go_up());
    wait_for_folder(&view, cx, &projects).await;
    cx.run_until_parked();
    assert_eq!(name(&view, cx).as_deref(), Some("mine.txt"));
}
