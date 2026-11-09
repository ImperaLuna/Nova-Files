//! The window's own keys, bound from its action table, and Back and
//! Forward by key, toolbar button and mouse side button: each test drives
//! a window on a real core and checks where it went.

use gpui::{MouseButton, NavigationDirection};

use super::*;

/// [`open_on`], with the window's keys bound as the program binds them.
async fn keyed<'a>(
    cx: &'a mut TestAppContext,
    folder: &Path,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    cx.update(nova_files_ui::bind_window_keys);
    let (view, cx) = open_on(cx, folder).await;
    cx.run_until_parked();
    (view, cx)
}

#[gpui::test]
async fn alt_up_goes_up(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("keys");
    let projects = tree.path().join("Projects");
    let (view, cx) = keyed(cx, &projects).await;

    cx.simulate_keystrokes("alt-up");
    wait_for_folder(&view, cx, tree.path()).await;
}

/// Goes into `Projects`, then `nova`, from the tree's top, by clicks.
async fn walk_in(view: &Entity<MainWindow>, cx: &mut VisualTestContext, tree: &TempFolder) {
    click(cx, "entry-0");
    wait_for_folder(view, cx, &tree.path().join("Projects")).await;
    click(cx, "entry-0");
    wait_for_folder(view, cx, &tree.path().join("Projects/nova")).await;
}

#[gpui::test]
async fn back_and_forward_retrace_the_walk_by_keys_and_buttons(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("keys");
    let (view, cx) = keyed(cx, tree.path()).await;
    walk_in(&view, cx, &tree).await;
    let projects = tree.path().join("Projects");

    cx.simulate_keystrokes("alt-left");
    wait_for_folder(&view, cx, &projects).await;
    cx.simulate_keystrokes("backspace");
    wait_for_folder(&view, cx, tree.path()).await;
    cx.simulate_keystrokes("alt-right");
    wait_for_folder(&view, cx, &projects).await;
    click(cx, "forward");
    wait_for_folder(&view, cx, &projects.join("nova")).await;
    click(cx, "back");
    wait_for_folder(&view, cx, &projects).await;
}

#[gpui::test]
async fn the_mouse_side_buttons_go_back_and_forward(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("keys");
    let (view, cx) = keyed(cx, tree.path()).await;
    walk_in(&view, cx, &tree).await;
    let projects = tree.path().join("Projects");
    cx.run_until_parked();
    let inside = cx.debug_bounds("modified-header").unwrap().center();

    cx.simulate_mouse_down(
        inside,
        MouseButton::Navigate(NavigationDirection::Back),
        Modifiers::none(),
    );
    wait_for_folder(&view, cx, &projects).await;
    cx.simulate_mouse_down(
        inside,
        MouseButton::Navigate(NavigationDirection::Forward),
        Modifiers::none(),
    );
    wait_for_folder(&view, cx, &projects.join("nova")).await;
}

#[gpui::test]
async fn a_new_place_clears_forward(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("keys");
    let (view, cx) = keyed(cx, tree.path()).await;
    walk_in(&view, cx, &tree).await;
    cx.simulate_keystrokes("alt-left");
    wait_for_folder(&view, cx, &tree.path().join("Projects")).await;
    view.read_with(cx, |view, _| assert!(view.forward_target().is_some()));

    cx.simulate_keystrokes("alt-up");
    wait_for_folder(&view, cx, tree.path()).await;
    view.read_with(cx, |view, _| assert_eq!(view.forward_target(), None));
}

#[gpui::test]
async fn a_place_gone_since_is_skipped_and_the_window_stays(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("keys");
    let (view, cx) = keyed(cx, tree.path()).await;
    walk_in(&view, cx, &tree).await;
    let projects = tree.path().join("Projects");
    cx.simulate_keystrokes("alt-up");
    wait_for_folder(&view, cx, &projects).await;
    fs::remove_dir(projects.join("nova")).unwrap();

    // Back would go to nova, which is gone: the window stays, and the
    // next Back goes on past it.
    cx.simulate_keystrokes("alt-left");
    view.condition::<WindowEvent>(cx, |view, _| !view.waiting())
        .await;
    view.read_with(cx, |view, _| {
        assert_eq!(view.folder(), Some(projects.as_path()));
        assert_eq!(view.back_target(), Some(tree.path()));
    });
    cx.simulate_keystrokes("alt-left");
    wait_for_folder(&view, cx, tree.path()).await;
}
