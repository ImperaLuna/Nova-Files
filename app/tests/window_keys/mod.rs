use gpui::{MouseButton, NavigationDirection};
use nova_files_api::HiddenFiles;

use super::*;

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

async fn walk_in(view: &Entity<MainWindow>, cx: &mut VisualTestContext, tree: &TempFolder) {
    double_click(cx, "entry-0");
    wait_for_folder(view, cx, &tree.path().join("Projects")).await;
    double_click(cx, "entry-0");
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

    cx.simulate_keystrokes("alt-left");
    view.condition::<WindowEvent>(cx, |view, _| !view.waiting())
        .await;
    view.read_with(cx, |view, _| {
        assert_eq!(view.folder(), Some(projects.as_path()));
        assert_eq!(view.back_target(), Some(&Location::from(tree.path())));
    });
    cx.simulate_keystrokes("alt-left");
    wait_for_folder(&view, cx, tree.path()).await;
}

#[gpui::test]
async fn ctrl_h_shows_hidden_files_and_leaves_them_out_again(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("keys-hidden");
    let projects = tree.path().join("Projects");
    fs::write(projects.join(".secret"), "").unwrap();
    cx.update(nova_files_ui::bind_window_keys);
    let (view, cx) = window_on(cx, &projects, tree.path());
    wait_for_folder(&view, cx, &projects).await;
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.entry_names()), ["nova"]);

    cx.simulate_keystrokes("ctrl-h");
    view.condition::<WindowEvent>(cx, |view, _| view.entry_names() == ["nova", ".secret"])
        .await;
    assert_eq!(
        cx.update(|_, cx| Published::<Settings>::get(cx).map(|settings| settings.hidden_files)),
        Some(HiddenFiles::Shown)
    );

    cx.simulate_keystrokes("ctrl-h");
    view.condition::<WindowEvent>(cx, |view, _| view.entry_names() == ["nova"])
        .await;
}
