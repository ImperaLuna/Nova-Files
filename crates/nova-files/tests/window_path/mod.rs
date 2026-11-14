//! The path bar: a crumb for each level from Home down to the folder on
//! screen, each going to its folder, and a narrow window keeping the
//! folder on screen in view.

use gpui::{px, size};

use super::*;

/// A window on `folder`, with the tree's top as the home folder, once the
/// folder is on screen.
async fn path_window<'a>(
    cx: &'a mut TestAppContext,
    tree: &TempFolder,
    folder: &Path,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    let (view, cx) = window_on(cx, folder, tree.path());
    wait_for_folder(&view, cx, folder).await;
    cx.run_until_parked();
    (view, cx)
}

/// Whether the element drawn with `selector` is on screen.
fn drawn(cx: &mut VisualTestContext, selector: &'static str) -> bool {
    cx.debug_bounds(selector).is_some()
}

#[gpui::test]
async fn each_crumb_goes_to_its_folder(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("path");
    let nova = tree.path().join("Projects/nova");
    let (view, cx) = path_window(cx, &tree, &nova).await;

    assert!(drawn(cx, "crumb-0"));
    assert!(drawn(cx, "crumb-2"));
    assert!(!drawn(cx, "crumb-3"));
    assert!(drawn(cx, "arrow-0"));
    assert!(drawn(cx, "arrow-1"));
    assert!(!drawn(cx, "arrow-2"));

    click(cx, "crumb-1");
    wait_for_folder(&view, cx, &tree.path().join("Projects")).await;
    click(cx, "crumb-0");
    wait_for_folder(&view, cx, tree.path()).await;
}

#[gpui::test]
async fn a_narrow_window_keeps_the_folder_on_screen_in_view(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("path");
    let deep = tree
        .path()
        .join("a long folder name/another long folder name/the folder");
    fs::create_dir_all(&deep).unwrap();
    let (_view, cx) = path_window(cx, &tree, &deep).await;

    cx.simulate_resize(size(px(560.0), px(400.0)));
    cx.run_until_parked();
    let path = cx.debug_bounds("path").unwrap();
    let here = cx.debug_bounds("crumb-3").unwrap();
    let home = cx.debug_bounds("crumb-0").unwrap();
    assert!(here.left() >= path.left() && here.right() <= path.right());
    // The root side gives way first.
    assert!(home.left() < path.left());
}
