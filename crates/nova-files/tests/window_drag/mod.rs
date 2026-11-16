//! Dragging files on a real core and a temp folder tree: a name dragged
//! onto a folder row moves the file there on one drive, Ctrl copies, the
//! island says so with Undo, and a drop that would change nothing (a
//! folder onto itself, files onto the folder they are in) does nothing.
//! The rules for every target are tested in the UI crate's `dropping`.

use gpui::{MouseButton, Pixels, Point, point, px};

use super::*;

/// A window on `folder`, with a home of its own in `home`, so the trash
/// and the core's state stay out of the folder on screen.
async fn drag_window<'a>(
    cx: &'a mut TestAppContext,
    folder: &Path,
    home: &TempFolder,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    cx.update(nova_files_ui::bind_window_keys);
    let (view, cx) = window_on(cx, folder, home.path());
    wait_for_folder(&view, cx, folder).await;
    cx.run_until_parked();
    (view, cx)
}

fn center(cx: &mut VisualTestContext, selector: &'static str) -> Point<Pixels> {
    cx.run_until_parked();
    cx.debug_bounds(selector).unwrap().center()
}

/// Presses on `from`, drags to `to` and lets go there with `modifiers`.
fn drag(cx: &mut VisualTestContext, from: &'static str, to: &'static str, modifiers: Modifiers) {
    let (start, end) = (center(cx, from), center(cx, to));
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        start + point(px(20.0), px(0.0)),
        Some(MouseButton::Left),
        Modifiers::none(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_move(end, Some(MouseButton::Left), modifiers);
    cx.simulate_modifiers_change(modifiers);
    cx.run_until_parked();
    cx.simulate_mouse_up(end, MouseButton::Left, modifiers);
    cx.simulate_modifiers_change(Modifiers::none());
    cx.run_until_parked();
}

/// Drags `from` onto `to` like [`drag`], checking the chip shows on
/// the way.
fn drag_checked(cx: &mut VisualTestContext, from: &'static str, to: &'static str) {
    let (start, end) = (center(cx, from), center(cx, to));
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
    cx.run_until_parked();
    assert!(cx.debug_bounds("drag-chip").is_some());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
}

async fn wait_for_note(view: &Entity<MainWindow>, cx: &VisualTestContext, label: &str) {
    view.condition::<WindowEvent>(cx, |view, _| {
        view.note().is_some_and(|note| note.label == label)
    })
    .await;
}

#[gpui::test]
async fn a_name_dragged_onto_a_folder_moves_it_there_and_undo_moves_it_back(
    cx: &mut TestAppContext,
) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("drag-move");
    let home = TempFolder::tree("drag-move-home");
    let (view, cx) = drag_window(cx, tree.path(), &home).await;
    let projects = tree.path().join("Projects");
    // Rows keep a clear drop edge inside their padding, so a name starts
    // where it always did: the row's pad, the icon and the gap in
    // (12 + 22 + 11 px).
    let inset =
        cx.debug_bounds("name-1").unwrap().left() - cx.debug_bounds("entry-1").unwrap().left();
    assert_eq!(inset, px(45.0));

    // Projects, notes.txt: nothing selected, so the drag selects it.
    drag_checked(cx, "name-1", "entry-0");
    wait_for_note(&view, cx, "Moved notes.txt to Projects").await;
    assert!(!tree.path().join("notes.txt").exists());
    assert!(projects.join("notes.txt").exists());

    click(cx, "island-undo");
    view.condition::<WindowEvent>(cx, |view, _| {
        view.entry_names() == ["Projects", "notes.txt"]
    })
    .await;
    assert!(!projects.join("notes.txt").exists());
}

#[gpui::test]
async fn ctrl_copies_what_is_dragged(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("drag-copy");
    let home = TempFolder::tree("drag-copy-home");
    let (view, cx) = drag_window(cx, tree.path(), &home).await;

    drag(cx, "name-1", "entry-0", Modifiers::control());
    wait_for_note(&view, cx, "Copied notes.txt to Projects").await;
    assert!(tree.path().join("notes.txt").exists());
    assert!(tree.path().join("Projects/notes.txt").exists());
}

#[gpui::test]
async fn a_drop_that_changes_nothing_does_nothing(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("drag-nothing");
    let home = TempFolder::tree("drag-nothing-home");
    let (view, cx) = drag_window(cx, tree.path(), &home).await;

    // A folder onto itself, then a file onto the folder it is in.
    drag(cx, "name-0", "entry-0", Modifiers::none());
    drag(cx, "name-1", "modified-1", Modifiers::none());
    view.read_with(cx, |view, _| {
        assert_eq!(view.jobs_pending(), 0);
        assert_eq!(view.entry_names(), ["Projects", "notes.txt"]);
    });
}

#[gpui::test]
async fn names_in_the_trash_do_not_drag(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("drag-in-trash");
    let home = TempFolder::tree("drag-in-trash-home");
    let (view, cx) = drag_window(cx, tree.path(), &home).await;
    click(cx, "name-1");
    cx.simulate_keystrokes("delete");
    wait_for_note(&view, cx, "Moved notes.txt to Trash").await;
    view.update(cx, MainWindow::open_trash);
    view.condition::<WindowEvent>(cx, |view, _| view.entry_names() == ["notes.txt"])
        .await;
    assert!(view.read_with(cx, |view, _| view.list_drop_folder().is_none()));

    // Pressed and moved, a name here carries no chip: nothing drags.
    let start = center(cx, "name-0");
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        start + point(px(40.0), px(0.0)),
        Some(MouseButton::Left),
        Modifiers::none(),
    );
    cx.run_until_parked();
    assert!(cx.debug_bounds("drag-chip").is_none());
    cx.simulate_mouse_up(start, MouseButton::Left, Modifiers::none());
}
