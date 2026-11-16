//! Dragging files on a real core and a temp folder tree: a name dragged
//! onto a folder row moves the file there on one drive, Ctrl copies, the
//! island says so with Undo, and a drop that would change nothing (a
//! folder onto itself, files onto the folder they are in) does nothing.
//! The trash, a tree folder, a crumb and a tab take drops too. On the way,
//! the chip names the drop under the pointer, and a tab or a sidebar row
//! held over opens, the drag going on.
//! The rules for every target are tested in the UI crate's `dropping`.

use gpui::{MouseButton, Pixels, Point, point, px};
use nova_files_ui::HOVER_OPEN_AFTER;

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
async fn the_chip_names_the_drop_under_the_pointer(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("drag-says");
    let home = TempFolder::tree("drag-says-home");
    let (view, cx) = drag_window(cx, tree.path(), &home).await;
    let says = |view: &Entity<MainWindow>, cx: &mut VisualTestContext, modifiers| {
        view.read_with(cx, |view, _| view.drop_says(modifiers))
    };
    let (start, file, folder, trash) = (
        center(cx, "name-1"),
        center(cx, "modified-1"),
        center(cx, "entry-0"),
        center(cx, "place-trash"),
    );
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(file, Some(MouseButton::Left), Modifiers::none());
    cx.run_until_parked();
    // Over its own row, in the folder it is already in: nothing to say.
    cx.simulate_mouse_move(
        file + point(px(1.0), px(0.0)),
        Some(MouseButton::Left),
        Modifiers::none(),
    );
    cx.run_until_parked();
    assert!(cx.debug_bounds("drag-chip").is_some());
    assert_eq!(says(&view, cx, Modifiers::none()), None);

    // Over a folder on the same drive: the core says it moves.
    cx.simulate_mouse_move(folder, Some(MouseButton::Left), Modifiers::none());
    view.condition::<WindowEvent>(cx, |view, _| {
        view.drop_says(Modifiers::none()).as_deref() == Some("Move to Projects")
    })
    .await;
    assert_eq!(
        says(&view, cx, Modifiers::control()).as_deref(),
        Some("Copy to Projects")
    );

    cx.simulate_mouse_move(trash, Some(MouseButton::Left), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        says(&view, cx, Modifiers::control()).as_deref(),
        Some("Move to Trash")
    );
    // Let go where nothing drops.
    cx.simulate_mouse_move(file, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(file, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.jobs_pending()), 0);
}

/// Presses on `from` and moves until it drags.
fn start_drag(cx: &mut VisualTestContext, from: &'static str) {
    let start = center(cx, from);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        start + point(px(20.0), px(0.0)),
        Some(MouseButton::Left),
        Modifiers::none(),
    );
    cx.run_until_parked();
}

/// Moves what is dragged over `to`, wiggling once as a hand does.
fn drag_over(cx: &mut VisualTestContext, to: &'static str) {
    let at = center(cx, to);
    cx.simulate_mouse_move(at, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_move(
        at + point(px(1.0), px(0.0)),
        Some(MouseButton::Left),
        Modifiers::none(),
    );
    cx.run_until_parked();
}

/// Holds what is dragged where it is for `wait`.
fn hold(cx: &VisualTestContext, wait: Duration) {
    cx.executor().advance_clock(wait);
    cx.run_until_parked();
}

#[gpui::test]
async fn a_tab_held_over_shows_and_the_drag_goes_on_into_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("drag-hover-tab");
    let home = TempFolder::tree("drag-hover-tab-home");
    let (view, cx) = drag_window(cx, tree.path(), &home).await;
    let projects = tree.path().join("Projects");
    view.update(cx, |view, cx| {
        view.open_in_new_tab(projects.clone(), cx);
        view.new_tab(&Location::Start, cx);
        view.switch_tab(0, cx);
    });
    wait_for_folder(&view, cx, tree.path()).await;
    let tab = |view: &Entity<MainWindow>, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| view.current_tab())
    };

    start_drag(cx, "name-1");
    // Passing over Start's tab, and away before the wait is up, does
    // nothing.
    drag_over(cx, "tab-2");
    hold(cx, HOVER_OPEN_AFTER / 2);
    drag_over(cx, "modified-0");
    hold(cx, HOVER_OPEN_AFTER);
    assert_eq!(tab(&view, cx), 0);

    drag_over(cx, "tab-1");
    hold(cx, HOVER_OPEN_AFTER);
    wait_for_folder(&view, cx, &projects).await;
    assert_eq!(tab(&view, cx), 1);
    // Projects holds nova: notes.txt goes into it.
    drag_over(cx, "entry-0");
    let end = center(cx, "entry-0");
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    wait_for_note(&view, cx, "Moved notes.txt to nova").await;
    assert!(projects.join("nova/notes.txt").exists());
}

#[gpui::test]
async fn start_and_places_held_over_open_in_the_tab_on_screen(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("drag-hover-place");
    let home = TempFolder::tree("drag-hover-place-home");
    let (view, cx) = drag_window(cx, tree.path(), &home).await;
    view.update(cx, |view, cx| view.new_tab(&Location::Start, cx));
    view.update(cx, |view, cx| view.switch_tab(0, cx));
    wait_for_folder(&view, cx, tree.path()).await;

    // Start's tab takes no files, but shows.
    start_drag(cx, "name-1");
    drag_over(cx, "tab-1");
    hold(cx, HOVER_OPEN_AFTER);
    assert!(view.read_with(cx, |view, _| view.current_tab() == 1 && view.is_on_start()));

    // The trash opens in it, and the drag still lives: let go there.
    drag_over(cx, "place-trash");
    hold(cx, HOVER_OPEN_AFTER);
    assert!(view.read_with(cx, |view, _| view.current_tab() == 1 && view.trash_open()));
    assert!(cx.debug_bounds("drag-chip").is_some());
    let end = center(cx, "place-trash");
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    wait_for_note(&view, cx, "Moved notes.txt to Trash").await;

    // Held over after the drag ended, nothing opens.
    hold(cx, HOVER_OPEN_AFTER);
    assert!(view.read_with(cx, |view, _| view.trash_open()));
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
async fn files_dropped_on_the_trash_go_to_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("drag-trash");
    let home = TempFolder::tree("drag-trash-home");
    let (view, cx) = drag_window(cx, tree.path(), &home).await;
    // An empty trash shows no count.
    view.condition::<WindowEvent>(cx, |view, _| view.trash_count() == Some(0))
        .await;
    cx.run_until_parked();
    assert!(cx.debug_bounds("place-trash-number").is_none());

    // Shift picks nothing for the trash.
    drag(cx, "name-1", "place-trash", Modifiers::shift());
    wait_for_note(&view, cx, "Moved notes.txt to Trash").await;
    view.condition::<WindowEvent>(cx, |view, _| view.trash_count() == Some(1))
        .await;
    cx.run_until_parked();
    assert!(cx.debug_bounds("place-trash-number").is_some());
    assert!(!tree.path().join("notes.txt").exists());
    assert!(
        home.path()
            .join(".local/share/Trash/files/notes.txt")
            .exists()
    );
}

#[gpui::test]
async fn files_drop_on_a_tree_folder_and_a_crumb(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("drag-sidebar");
    let projects = tree.path().join("Projects");
    cx.update(nova_files_ui::bind_window_keys);
    let (view, cx) = window_on(cx, tree.path(), tree.path());
    wait_for_folder(&view, cx, tree.path()).await;
    click(cx, "tree-toggle-0");
    view.condition::<WindowEvent>(cx, |view, _| view.tree_settled())
        .await;

    // Home holds Projects, notes.txt; the tree Home, Projects.
    drag(cx, "name-1", "tree-1", Modifiers::none());
    wait_for_note(&view, cx, "Moved notes.txt to Projects").await;
    assert!(projects.join("notes.txt").exists());

    // In Projects (nova, notes.txt), nova up onto Home's crumb.
    click(cx, "tree-1");
    wait_for_folder(&view, cx, &projects).await;
    drag(cx, "name-0", "crumb-0", Modifiers::none());
    view.condition::<WindowEvent>(cx, |view, _| view.entry_names() == ["notes.txt"])
        .await;
    assert!(tree.path().join("nova").is_dir());
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

#[gpui::test]
async fn files_dropped_on_a_tab_go_into_its_folder_or_the_trash(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("drag-tab");
    let home = TempFolder::tree("drag-tab-home");
    let (view, cx) = drag_window(cx, tree.path(), &home).await;
    let projects = tree.path().join("Projects");
    view.update(cx, |view, cx| {
        view.open_in_new_tab(projects.clone(), cx);
        view.new_tab(&Location::Start, cx);
        view.open_trash_in_new_tab(cx);
        view.switch_tab(0, cx);
    });
    wait_for_folder(&view, cx, tree.path()).await;

    // Start takes nothing, nor the tab whose folder the file is in.
    drag(cx, "name-1", "tab-2", Modifiers::none());
    drag(cx, "name-1", "tab-0", Modifiers::none());
    assert_eq!(view.read_with(cx, |view, _| view.jobs_pending()), 0);
    assert!(tree.path().join("notes.txt").exists());

    drag(cx, "name-1", "tab-1", Modifiers::none());
    wait_for_note(&view, cx, "Moved notes.txt to Projects").await;
    assert!(projects.join("notes.txt").exists());

    // The trash's tab sends what is dropped on it to the trash.
    drag(cx, "name-0", "tab-3", Modifiers::none());
    wait_for_note(&view, cx, "Moved Projects to Trash").await;
    assert!(!projects.exists());
}
