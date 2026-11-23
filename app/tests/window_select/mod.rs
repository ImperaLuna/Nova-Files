use super::*;

async fn listed<'a>(
    cx: &'a mut TestAppContext,
    tree: &TempFolder,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    fs::write(tree.path().join("a.txt"), "").unwrap();
    fs::write(tree.path().join("b.txt"), "").unwrap();
    cx.update(nova_files_window::bind_window_keys);
    let (view, cx) = open_on(cx, tree.path()).await;
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.entry_names(),
            ["Projects", "a.txt", "b.txt", "notes.txt"]
        );
    });
    (view, cx)
}

fn selected(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, _| view.selected_names())
}

#[gpui::test]
async fn arrows_move_and_shift_extends(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("select");
    let (view, cx) = listed(cx, &tree).await;

    cx.simulate_keystrokes("down");
    assert_eq!(selected(&view, cx), ["Projects"]);
    cx.simulate_keystrokes("down down");
    assert_eq!(selected(&view, cx), ["b.txt"]);
    cx.simulate_keystrokes("shift-up");
    assert_eq!(selected(&view, cx), ["a.txt", "b.txt"]);
    cx.simulate_keystrokes("shift-end");
    assert_eq!(selected(&view, cx), ["b.txt", "notes.txt"]);
    cx.simulate_keystrokes("home");
    assert_eq!(selected(&view, cx), ["Projects"]);
    cx.simulate_keystrokes("end");
    assert_eq!(selected(&view, cx), ["notes.txt"]);
    view.read_with(cx, |view, _| assert_eq!(view.cursor(), Some(3)));
}

#[gpui::test]
async fn select_all_toggle_and_escape(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("select");
    let (view, cx) = listed(cx, &tree).await;

    cx.simulate_keystrokes("ctrl-a");
    assert_eq!(selected(&view, cx).len(), 4);
    cx.simulate_keystrokes("ctrl-space");
    assert_eq!(selected(&view, cx), ["a.txt", "b.txt", "notes.txt"]);
    cx.simulate_keystrokes("escape");
    assert_eq!(selected(&view, cx), Vec::<String>::new());
}

#[gpui::test]
async fn ctrl_and_shift_clicks_select_several(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("select");
    let (view, cx) = listed(cx, &tree).await;

    click(cx, "entry-1");
    click_with(cx, "entry-3", Modifiers::control());
    assert_eq!(selected(&view, cx), ["a.txt", "notes.txt"]);
    click_with(cx, "entry-0", Modifiers::shift());
    assert_eq!(
        selected(&view, cx),
        ["Projects", "a.txt", "b.txt", "notes.txt"]
    );
    click(cx, "entry-2");
    assert_eq!(selected(&view, cx), ["b.txt"]);
}

#[gpui::test]
async fn enter_goes_into_the_folder_selected(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("select");
    let (view, cx) = listed(cx, &tree).await;

    cx.simulate_keystrokes("down enter");
    wait_for_folder(&view, cx, &tree.path().join("Projects")).await;
    assert_eq!(selected(&view, cx), Vec::<String>::new());
}

#[gpui::test]
async fn up_and_back_select_the_folder_left(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("select");
    let (view, cx) = listed(cx, &tree).await;
    let projects = tree.path().join("Projects");
    double_click(cx, "entry-0");
    wait_for_folder(&view, cx, &projects).await;

    cx.simulate_keystrokes("alt-up");
    wait_for_folder(&view, cx, tree.path()).await;
    assert_eq!(selected(&view, cx), ["Projects"]);

    cx.simulate_keystrokes("enter");
    wait_for_folder(&view, cx, &projects).await;
    cx.simulate_keystrokes("alt-left");
    wait_for_folder(&view, cx, tree.path()).await;
    assert_eq!(selected(&view, cx), ["Projects"]);
    cx.simulate_keystrokes("down");
    assert_eq!(selected(&view, cx), ["a.txt"]);
}

#[gpui::test]
async fn a_window_opened_to_show_files_has_them_selected(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("select");
    fs::write(tree.path().join("a.txt"), "").unwrap();
    fs::write(tree.path().join("b.txt"), "").unwrap();
    cx.update(nova_files_window::bind_window_keys);
    let names = ["notes.txt", "gone.txt", "a.txt"].map(Into::into).to_vec();
    let (view, cx) = window_showing(cx, tree.path(), tree.path(), names);
    wait_for_folder(&view, cx, tree.path()).await;
    cx.run_until_parked();

    assert_eq!(selected(&view, cx), ["a.txt", "notes.txt"]);
    view.read_with(cx, |view, _| assert_eq!(view.cursor(), Some(1)));
    cx.simulate_keystrokes("down");
    assert_eq!(selected(&view, cx), ["b.txt"]);

    cx.simulate_keystrokes("home enter");
    wait_for_folder(&view, cx, &tree.path().join("Projects")).await;
    cx.simulate_keystrokes("alt-left");
    wait_for_folder(&view, cx, tree.path()).await;
    assert_eq!(selected(&view, cx), ["Projects"]);
}

#[gpui::test]
async fn files_to_show_select_nothing_in_the_folder_fallen_back_to(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("select");
    let gone = tree.path().join("gone");
    let (view, cx) = window_showing(cx, &gone, tree.path(), vec!["notes.txt".into()]);

    wait_for_folder(&view, cx, tree.path()).await;
    cx.run_until_parked();
    assert_eq!(selected(&view, cx), Vec::<String>::new());
}

fn press(
    cx: &mut VisualTestContext,
    selector: &'static str,
    modifiers: Modifiers,
) -> gpui::Point<gpui::Pixels> {
    cx.run_until_parked();
    let at = cx.debug_bounds(selector).unwrap().center();
    cx.simulate_mouse_down(at, gpui::MouseButton::Left, modifiers);
    cx.run_until_parked();
    at
}

fn release(cx: &mut VisualTestContext, at: gpui::Point<gpui::Pixels>, modifiers: Modifiers) {
    cx.simulate_mouse_up(at, gpui::MouseButton::Left, modifiers);
    cx.run_until_parked();
}

#[gpui::test]
async fn a_press_lights_a_row_before_the_button_comes_up(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("select-press");
    let (view, cx) = listed(cx, &tree).await;

    let at = press(cx, "entry-1", Modifiers::none());
    assert_eq!(selected(&view, cx), ["a.txt"]);
    release(cx, at, Modifiers::none());
    assert_eq!(selected(&view, cx), ["a.txt"]);

    let at = press(cx, "entry-3", Modifiers::control());
    assert_eq!(selected(&view, cx), ["a.txt", "notes.txt"]);
    release(cx, at, Modifiers::control());
    assert_eq!(selected(&view, cx), ["a.txt", "notes.txt"]);
}

#[gpui::test]
async fn a_press_on_one_of_several_keeps_them_until_the_button_comes_up(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("select-keep");
    let (view, cx) = listed(cx, &tree).await;
    click(cx, "entry-1");
    click_with(cx, "entry-3", Modifiers::control());

    let at = press(cx, "entry-1", Modifiers::none());
    assert_eq!(selected(&view, cx), ["a.txt", "notes.txt"]);
    release(cx, at, Modifiers::none());
    assert_eq!(selected(&view, cx), ["a.txt"]);

    click_with(cx, "entry-3", Modifiers::control());
    let at = press(cx, "entry-3", Modifiers::control());
    assert_eq!(selected(&view, cx), ["a.txt", "notes.txt"]);
    release(cx, at, Modifiers::control());
    assert_eq!(selected(&view, cx), ["a.txt"]);
}

#[gpui::test]
async fn a_third_quick_click_in_the_folder_opened_only_selects(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("select-third");
    let (view, cx) = listed(cx, &tree).await;
    let projects = tree.path().join("Projects");

    let at = cx.debug_bounds("entry-0").unwrap().center();
    counted_click(cx, at, 1);
    counted_click(cx, at, 2);
    wait_for_folder(&view, cx, &projects).await;
    cx.run_until_parked();
    counted_click(cx, at, 3);
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.folder().map(Path::to_path_buf)),
        Some(projects)
    );
    assert_eq!(selected(&view, cx), ["nova"]);
}
