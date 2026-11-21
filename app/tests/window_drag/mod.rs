use gpui::{ExternalDragPayload, MouseButton, Pixels, Point, point, px};
use nova_files_window::HOVER_OPEN_AFTER;

use super::*;

async fn drag_window<'a>(
    cx: &'a mut TestAppContext,
    folder: &Path,
    home: &TempFolder,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    cx.update(nova_files_window::bind_window_keys);
    let (view, cx) = window_on(cx, folder, home.path());
    wait_for_folder(&view, cx, folder).await;
    cx.run_until_parked();
    (view, cx)
}

fn center(cx: &mut VisualTestContext, selector: &'static str) -> Point<Pixels> {
    cx.run_until_parked();
    cx.debug_bounds(selector).unwrap().center()
}

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
    let inset =
        cx.debug_bounds("name-1").unwrap().left() - cx.debug_bounds("entry-1").unwrap().left();
    assert_eq!(inset, px(45.0));

    drag_checked(cx, "name-1", "entry-0");
    wait_for_note(&view, cx, "Moved notes.txt to Projects").await;
    assert!(!tree.path().join("notes.txt").exists());
    assert!(projects.join("notes.txt").exists());
    view.condition::<WindowEvent>(cx, |view, _| view.entry_names() == ["Projects"])
        .await;

    click(cx, "island-undo");
    view.condition::<WindowEvent>(cx, |view, _| {
        view.entry_names() == ["Projects", "notes.txt"]
    })
    .await;
    assert!(!projects.join("notes.txt").exists());
}

#[gpui::test]
async fn a_drag_offers_its_files_and_a_pile_to_other_apps(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("drag-out");
    let home = TempFolder::tree("drag-out-home");
    let (view, cx) = drag_window(cx, tree.path(), &home).await;
    let (start, beside) = (center(cx, "name-1"), center(cx, "modified-1"));
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(beside, Some(MouseButton::Left), Modifiers::none());
    cx.run_until_parked();
    cx.simulate_mouse_move(
        beside + point(px(1.0), px(0.0)),
        Some(MouseButton::Left),
        Modifiers::none(),
    );
    cx.run_until_parked();

    let Some(ExternalDragPayload::Files(paths)) = view.read_with(cx, |view, _| view.drag_out())
    else {
        panic!("the drag offers no files");
    };
    assert_eq!(paths.entries(), [(tree.path().join("notes.txt"), false)]);
    assert!(paths.picture().is_some_and(|picture| picture.scale >= 1));
    cx.simulate_mouse_up(beside, MouseButton::Left, Modifiers::none());
}

#[gpui::test]
async fn the_chip_names_the_drop_under_the_pointer(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("drag-says");
    let home = TempFolder::tree("drag-says-home");
    let (view, cx) = drag_window(cx, tree.path(), &home).await;
    let says = |view: &Entity<MainWindow>, cx: &mut VisualTestContext, modifiers| {
        view.read_with(cx, |view, cx| view.drop_says(modifiers, cx))
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
    cx.simulate_mouse_move(
        file + point(px(1.0), px(0.0)),
        Some(MouseButton::Left),
        Modifiers::none(),
    );
    cx.run_until_parked();
    assert!(cx.debug_bounds("drag-chip").is_some());
    assert_eq!(says(&view, cx, Modifiers::none()), None);

    cx.simulate_mouse_move(folder, Some(MouseButton::Left), Modifiers::none());
    view.condition::<WindowEvent>(cx, |view, cx| {
        view.drop_says(Modifiers::none(), cx).as_deref() == Some("Move to Projects")
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
    cx.simulate_mouse_move(file, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(file, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.jobs_pending()), 0);
}

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
        view.new_tab(&Location::start(), cx);
        view.switch_tab(0, cx);
    });
    wait_for_folder(&view, cx, tree.path()).await;
    let tab = |view: &Entity<MainWindow>, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| view.current_tab())
    };

    start_drag(cx, "name-1");
    drag_over(cx, "tab-2");
    hold(cx, HOVER_OPEN_AFTER / 2);
    drag_over(cx, "modified-0");
    hold(cx, HOVER_OPEN_AFTER);
    assert_eq!(tab(&view, cx), 0);

    drag_over(cx, "tab-1");
    hold(cx, HOVER_OPEN_AFTER);
    wait_for_folder(&view, cx, &projects).await;
    assert_eq!(tab(&view, cx), 1);
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
    view.update(cx, |view, cx| view.new_tab(&Location::start(), cx));
    view.update(cx, |view, cx| view.switch_tab(0, cx));
    wait_for_folder(&view, cx, tree.path()).await;

    start_drag(cx, "name-1");
    drag_over(cx, "place-start");
    hold(cx, HOVER_OPEN_AFTER);
    assert!(view.read_with(cx, |view, _| view.current_tab() == 0 && view.is_on_start()));
    drag_over(cx, "tab-1");
    hold(cx, HOVER_OPEN_AFTER);
    assert!(view.read_with(cx, |view, _| view.current_tab() == 1 && view.is_on_start()));

    drag_over(cx, "place-trash");
    hold(cx, HOVER_OPEN_AFTER);
    assert!(view.read_with(cx, |view, cx| view.current_tab() == 1
        && trash_open(view, cx)));
    assert!(cx.debug_bounds("drag-chip").is_some());
    let end = center(cx, "place-trash");
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    wait_for_note(&view, cx, "Moved notes.txt to Trash").await;

    hold(cx, HOVER_OPEN_AFTER);
    assert!(view.read_with(cx, trash_open));
}

#[gpui::test]
async fn a_row_left_for_outside_the_window_opens_nothing(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("drag-hover-leave");
    let home = TempFolder::tree("drag-hover-leave-home");
    let (view, cx) = drag_window(cx, tree.path(), &home).await;

    start_drag(cx, "name-1");
    drag_over(cx, "place-trash");
    let outside = point(px(-40.0), center(cx, "place-trash").y);
    cx.simulate_mouse_move(outside, Some(MouseButton::Left), Modifiers::none());
    hold(cx, HOVER_OPEN_AFTER);
    assert!(view.read_with(cx, |view, cx| !trash_open(view, cx)));
    cx.simulate_mouse_up(outside, MouseButton::Left, Modifiers::none());
}

#[gpui::test]
async fn the_chip_names_the_folder_on_screen_over_its_list(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("drag-says-list");
    let home = TempFolder::tree("drag-says-list-home");
    let (view, cx) = drag_window(cx, tree.path(), &home).await;
    let projects = tree.path().join("Projects");
    let start = center(cx, "name-1");
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        start + point(px(20.0), px(0.0)),
        Some(MouseButton::Left),
        Modifiers::none(),
    );
    cx.run_until_parked();
    view.update(cx, |view, cx| {
        view.open(&projects);
        cx.notify();
    });
    wait_for_folder(&view, cx, &projects).await;
    let below = cx.debug_bounds("entry-0").unwrap().bottom_left() + point(px(40.0), px(60.0));
    cx.simulate_mouse_move(below, Some(MouseButton::Left), Modifiers::none());
    view.condition::<WindowEvent>(cx, |view, cx| {
        view.drop_says(Modifiers::none(), cx).as_deref() == Some("Move to Projects")
    })
    .await;
    cx.simulate_mouse_up(below, MouseButton::Left, Modifiers::none());
    wait_for_note(&view, cx, "Moved notes.txt to Projects").await;
    assert!(projects.join("notes.txt").exists());
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
    view.condition::<WindowEvent>(cx, |view, cx| trash_count(view, cx) == Some(0))
        .await;
    cx.run_until_parked();
    assert!(cx.debug_bounds("place-trash-number").is_none());

    drag(cx, "name-1", "place-trash", Modifiers::shift());
    wait_for_note(&view, cx, "Moved notes.txt to Trash").await;
    view.condition::<WindowEvent>(cx, |view, cx| trash_count(view, cx) == Some(1))
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
    cx.update(nova_files_window::bind_window_keys);
    let (view, cx) = window_on(cx, tree.path(), tree.path());
    wait_for_folder(&view, cx, tree.path()).await;
    click(cx, "tree-0-expander");
    view.condition::<WindowEvent>(cx, |view, cx| tree_part(view).read(cx).settled())
        .await;

    drag(cx, "name-1", "tree-1", Modifiers::none());
    wait_for_note(&view, cx, "Moved notes.txt to Projects").await;
    assert!(projects.join("notes.txt").exists());

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
    open_trash(&view, cx, false).await;
    view.condition::<WindowEvent>(cx, |view, _| view.entry_names() == ["notes.txt"])
        .await;
    assert!(view.read_with(cx, |view, _| view.list_drop_folder().is_none()));

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
        view.new_tab(&Location::start(), cx);
    });
    open_trash(&view, cx, true).await;
    view.update(cx, |view, cx| view.switch_tab(0, cx));
    wait_for_folder(&view, cx, tree.path()).await;

    drag(cx, "name-1", "tab-2", Modifiers::none());
    drag(cx, "name-1", "tab-0", Modifiers::none());
    assert_eq!(view.read_with(cx, |view, _| view.jobs_pending()), 0);
    assert!(tree.path().join("notes.txt").exists());

    drag(cx, "name-1", "tab-1", Modifiers::none());
    wait_for_note(&view, cx, "Moved notes.txt to Projects").await;
    assert!(projects.join("notes.txt").exists());

    drag(cx, "name-0", "tab-3", Modifiers::none());
    wait_for_note(&view, cx, "Moved Projects to Trash").await;
    assert!(!projects.exists());
}
