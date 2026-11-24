use super::*;
use gpui::{ScrollDelta, ScrollEvent, point};

fn field_state(
    view: &Entity<MainWindow>,
    cx: &VisualTestContext,
    index: usize,
) -> (String, std::ops::Range<usize>) {
    view.read_with(cx, |view, cx| {
        let field = view.rename_field(index, cx).unwrap();
        let field = field.read(cx);
        (field.text().to_owned(), field.selection())
    })
}

#[gpui::test]
async fn f2_renames_in_place_and_undo_names_it_back(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("rename");
    let home = TempFolder::tree("rename-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    click(cx, "name-1");
    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    assert_eq!(field_state(&view, cx, 1), ("notes.txt".to_owned(), 0..5));
    cx.simulate_input("plans");
    cx.simulate_keystrokes("enter");
    wait_for_names(&view, cx, &["Projects", "plans.txt"]).await;
    assert!(tree.path().join("plans.txt").exists());
    assert!(!view.read_with(cx, MainWindow::renaming));
    assert_eq!(
        view.read_with(cx, |view, _| view.selected_names()),
        ["plans.txt"]
    );
    wait_for_note(&view, cx, "Renamed to plans.txt").await;

    click(cx, "island-undo");
    wait_for_names(&view, cx, &["Projects", "notes.txt"]).await;
}

#[gpui::test]
async fn shift_delete_in_the_field_cuts_text_not_files(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("rename");
    let home = TempFolder::tree("rename-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    click(cx, "name-1");
    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    cx.simulate_keystrokes("shift-delete");
    cx.run_until_parked();
    assert_eq!(field_state(&view, cx, 1), (".txt".to_owned(), 0..0));
    assert!(view.read_with(cx, |view, _| view.confirm().is_none()));
    assert!(view.read_with(cx, MainWindow::renaming));
}

#[gpui::test]
async fn escape_or_the_same_name_leaves_it_as_it_was(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("rename");
    let home = TempFolder::tree("rename-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    click(cx, "name-0");
    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    assert_eq!(field_state(&view, cx, 0), ("Projects".to_owned(), 0..8));
    cx.simulate_input("Elsewhere");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!view.read_with(cx, MainWindow::renaming));
    assert_eq!(
        view.read_with(cx, |view, _| view.selected_names()),
        ["Projects"]
    );

    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(!view.read_with(cx, MainWindow::renaming));
    assert!(view.read_with(cx, |view, _| view.note().is_none()));
    assert!(tree.path().join("Projects").is_dir());
}

#[gpui::test]
async fn rename_needs_one_entry_selected(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("rename");
    let home = TempFolder::tree("rename-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    cx.simulate_keystrokes("ctrl-a");
    cx.run_until_parked();
    assert!(cx.debug_bounds("island-rename").is_none());
    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    assert!(!view.read_with(cx, MainWindow::renaming));
}

#[gpui::test]
async fn new_folder_makes_one_and_starts_renaming_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("rename");
    let home = TempFolder::tree("rename-home");
    fs::create_dir(tree.path().join("New folder")).unwrap();
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    cx.simulate_keystrokes("ctrl-shift-n");
    wait_for_names(
        &view,
        cx,
        &["New folder", "New folder 2", "Projects", "notes.txt"],
    )
    .await;
    cx.run_until_parked();
    assert_eq!(
        field_state(&view, cx, 1),
        ("New folder 2".to_owned(), 0..12)
    );
    cx.simulate_input("Music");
    cx.simulate_keystrokes("enter");
    wait_for_names(&view, cx, &["Music", "New folder", "Projects", "notes.txt"]).await;
    assert!(tree.path().join("Music").is_dir());
}

#[gpui::test]
async fn a_double_click_in_the_field_stays_in_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("rename-press");
    let home = TempFolder::tree("rename-press-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    click(cx, "name-0");
    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    let field = cx.debug_bounds("rename").unwrap();
    let name = cx.debug_bounds("name-1").unwrap();
    assert!(field.left() < name.left());
    double_click(cx, "rename");
    cx.run_until_parked();
    assert!(view.read_with(cx, MainWindow::renaming));
    assert_eq!(
        view.read_with(cx, |view, _| view.entry_names()),
        ["Projects", "notes.txt"]
    );
    cx.dispatch_action(nova_files_rename_ui::NewFolder);
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.jobs_pending()), 0);
}

#[gpui::test]
async fn a_rename_that_moves_the_entry_off_screen_scrolls_to_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("rename-far");
    let home = TempFolder::tree("rename-far-home");
    for index in 0..80 {
        fs::write(tree.path().join(format!("file {index:02}")), "").unwrap();
    }
    let (view, cx) = island_window(cx, tree.path(), &home).await;
    let offset = |view: &Entity<MainWindow>, cx: &VisualTestContext| {
        view.read_with(cx, |view, _| view.scroll().0.borrow().base_handle.offset())
    };
    let top = offset(&view, cx);

    click(cx, "name-2");
    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    cx.simulate_input("zz last");
    cx.simulate_keystrokes("enter");
    view.condition::<WindowEvent>(cx, |view, _| {
        view.entry_names().last().map(String::as_str) == Some("zz last")
    })
    .await;
    cx.run_until_parked();
    assert!(offset(&view, cx).y < top.y);
}

#[gpui::test]
async fn a_name_typed_then_scrolled_away_is_kept(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("rename-scrolled");
    let home = TempFolder::tree("rename-scrolled-home");
    for index in 0..80 {
        fs::write(tree.path().join(format!("file {index:02}")), "").unwrap();
    }
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    click(cx, "name-2");
    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    cx.simulate_input("kept");
    let list = cx.debug_bounds("entry-5").unwrap().center();
    cx.simulate_event(ScrollEvent {
        position: list,
        delta: ScrollDelta::Pixels(point(px(0.0), px(-2000.0))),
        ..ScrollEvent::default()
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("entry-2").is_none());
    view.condition::<WindowEvent>(cx, |view, _| {
        view.entry_names().iter().any(|name| name == "kept")
    })
    .await;
    assert!(!view.read_with(cx, MainWindow::renaming));
    assert!(!tree.path().join("file 01").exists());
}

#[gpui::test]
async fn a_triple_click_in_the_field_selects_the_whole_name(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("rename-triple");
    let home = TempFolder::tree("rename-triple-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    click(cx, "name-1");
    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    let field = cx.debug_bounds("rename").unwrap();
    let at = point(field.left() + px(12.0), field.center().y);
    for count in 1..=2 {
        counted_click(cx, at, count);
    }
    cx.run_until_parked();
    assert_eq!(field_state(&view, cx, 1), ("notes.txt".to_owned(), 0..5));
    counted_click(cx, at, 3);
    cx.run_until_parked();
    assert_eq!(field_state(&view, cx, 1), ("notes.txt".to_owned(), 0..9));
}

#[gpui::test]
async fn the_mouse_back_button_keeps_a_name_being_typed(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("rename-back");
    let home = TempFolder::tree("rename-back-home");
    let projects = tree.path().join("Projects");
    let (view, cx) = island_window(cx, tree.path(), &home).await;
    view.update(cx, |view, _| view.open(&projects));
    wait_for_folder(&view, cx, &projects).await;
    cx.run_until_parked();

    click(cx, "name-0");
    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    cx.simulate_input("kept");
    let at = cx.debug_bounds("entry-0").unwrap().center();
    cx.simulate_event(gpui::MouseDownEvent {
        position: at,
        modifiers: Modifiers::none(),
        button: gpui::MouseButton::Navigate(gpui::NavigationDirection::Back),
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(gpui::MouseUpEvent {
        position: at,
        modifiers: Modifiers::none(),
        button: gpui::MouseButton::Navigate(gpui::NavigationDirection::Back),
        click_count: 1,
    });
    cx.run_until_parked();
    view.condition::<WindowEvent>(cx, |_, _| projects.join("kept").exists())
        .await;
}

fn click_at(cx: &mut VisualTestContext, at: gpui::Point<gpui::Pixels>, count: usize) {
    counted_click(cx, at, count);
    cx.run_until_parked();
}

fn wait(cx: &VisualTestContext) {
    cx.executor()
        .advance_clock(nova_files_window::RENAME_AFTER * 2);
    cx.run_until_parked();
}

#[gpui::test]
async fn a_slow_second_click_on_the_name_starts_a_rename(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("slow-rename");
    let home = TempFolder::tree("slow-rename-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;
    let name = cx.debug_bounds("name-1").unwrap().center();

    click_at(cx, name, 1);
    wait(cx);
    assert!(!view.read_with(cx, MainWindow::renaming));
    click_at(cx, name, 1);
    assert!(
        !view.read_with(cx, MainWindow::renaming),
        "it waits for a double-click first"
    );
    wait(cx);
    assert_eq!(field_state(&view, cx, 1), ("notes.txt".to_owned(), 0..5));
}

#[gpui::test]
async fn a_double_click_on_a_selected_name_opens_and_never_renames(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("slow-rename-double");
    let home = TempFolder::tree("slow-rename-double-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;
    let name = cx.debug_bounds("name-0").unwrap().center();

    click_at(cx, name, 1);
    wait(cx);
    click_at(cx, name, 1);
    click_at(cx, name, 2);
    wait(cx);
    assert!(!view.read_with(cx, MainWindow::renaming));
    let here = view.read_with(cx, |view, _| view.folder().map(Path::to_path_buf));
    assert_eq!(here, Some(tree.path().join("Projects")));
}

#[gpui::test]
async fn a_second_click_on_the_icon_does_not_rename(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("slow-rename-icon");
    let home = TempFolder::tree("slow-rename-icon-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;
    let name = cx.debug_bounds("name-1").unwrap();
    let icon = point(name.left() - px(16.0), name.center().y);

    click_at(cx, name.center(), 1);
    wait(cx);
    click_at(cx, icon, 1);
    wait(cx);
    assert!(!view.read_with(cx, MainWindow::renaming));
    assert_eq!(
        view.read_with(cx, |view, _| view.selected_names()),
        ["notes.txt"]
    );
}

#[gpui::test]
async fn duplicate_copies_beside_the_original_and_undo_takes_the_copy_back(
    cx: &mut TestAppContext,
) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("duplicate");
    let home = TempFolder::tree("duplicate-home");
    fs::write(tree.path().join("notes.txt"), "mine").unwrap();
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    click(cx, "name-1");
    cx.simulate_keystrokes("ctrl-d");
    wait_for_names(&view, cx, &["Projects", "notes (2).txt", "notes.txt"]).await;
    assert_eq!(
        fs::read_to_string(tree.path().join("notes (2).txt")).unwrap(),
        "mine"
    );
    cx.simulate_keystrokes("ctrl-z");
    wait_for_names(&view, cx, &["Projects", "notes.txt"]).await;

    cx.simulate_keystrokes("f2 home ctrl-d enter");
    wait_for_names(&view, cx, &["Projects", "otes.txt"]).await;
}
