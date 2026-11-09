//! The window's own keys, bound from its action table: each test presses
//! keys in a window on a real core and checks where it went.

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
