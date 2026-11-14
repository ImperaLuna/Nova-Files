//! The path bar: a crumb for each level from Home down to the folder on
//! screen, each going to its folder; an arrow after each opening a menu
//! of the folders in it; a long path folding its middle into a "…" menu;
//! and a narrow window keeping the folder on screen in view.

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

/// Waits until the path bar's menu holds names, and returns them.
async fn menu_names(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Vec<String> {
    view.condition::<WindowEvent>(cx, |view, _| {
        view.path_menu_names()
            .is_some_and(|names| !names.is_empty())
    })
    .await;
    cx.run_until_parked();
    view.read_with(cx, |view, _| view.path_menu_names().unwrap())
}

/// Waits until the path bar's menu has closed.
async fn menu_closed(view: &Entity<MainWindow>, cx: &mut VisualTestContext) {
    view.condition::<WindowEvent>(cx, |view, _| view.path_menu_names().is_none())
        .await;
    cx.run_until_parked();
    assert!(!drawn(cx, "path-menu"));
}

#[gpui::test]
async fn an_arrow_lists_the_folders_in_the_crumb_before_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("path");
    fs::create_dir(tree.path().join("Music")).unwrap();
    let nova = tree.path().join("Projects/nova");
    let (view, cx) = path_window(cx, &tree, &nova).await;

    click(cx, "arrow-0");
    assert_eq!(menu_names(&view, cx).await, ["Music", "Projects"]);
    assert!(drawn(cx, "path-menu-1"));
    click(cx, "path-menu-0");
    wait_for_folder(&view, cx, &tree.path().join("Music")).await;
    menu_closed(&view, cx).await;
}

#[gpui::test]
async fn a_press_on_an_open_arrow_closes_its_menu(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("path");
    let nova = tree.path().join("Projects/nova");
    let (view, cx) = path_window(cx, &tree, &nova).await;

    click(cx, "arrow-1");
    assert_eq!(menu_names(&view, cx).await, ["nova"]);
    click(cx, "arrow-1");
    menu_closed(&view, cx).await;
    // Another arrow opens its own menu in its place.
    click(cx, "arrow-1");
    menu_names(&view, cx).await;
    click(cx, "arrow-0");
    assert_eq!(menu_names(&view, cx).await, ["Projects"]);
}

#[gpui::test]
async fn the_folder_on_screen_has_an_arrow_only_when_it_holds_folders(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("path");
    let projects = tree.path().join("Projects");
    let (view, cx) = path_window(cx, &tree, &projects).await;

    assert!(drawn(cx, "arrow-1"));
    click(cx, "arrow-1");
    assert_eq!(menu_names(&view, cx).await, ["nova"]);
    click(cx, "path-menu-0");
    wait_for_folder(&view, cx, &projects.join("nova")).await;
    cx.run_until_parked();
    assert!(!drawn(cx, "arrow-2"));
}

#[gpui::test]
async fn a_menu_whose_folder_is_gone_or_holds_no_folders_closes(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("path");
    let deep = tree.path().join("a/b/c");
    fs::create_dir_all(&deep).unwrap();
    let (view, cx) = path_window(cx, &tree, &deep).await;

    fs::remove_dir(&deep).unwrap();
    click(cx, "arrow-2");
    menu_closed(&view, cx).await;
    fs::remove_dir_all(tree.path().join("a")).unwrap();
    click(cx, "arrow-1");
    menu_closed(&view, cx).await;
}

#[gpui::test]
async fn a_long_path_folds_its_middle_into_a_menu(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("path");
    let deep = tree.path().join("a/b/c/d/e/f");
    fs::create_dir_all(&deep).unwrap();
    let (view, cx) = path_window(cx, &tree, &deep).await;

    // Home, …, d, e, f.
    for shown in ["crumb-0", "crumb-more", "crumb-4", "crumb-5", "crumb-6"] {
        assert!(drawn(cx, shown), "{shown} not drawn");
    }
    for folded in ["crumb-1", "crumb-2", "crumb-3", "arrow-1", "arrow-3"] {
        assert!(!drawn(cx, folded), "{folded} drawn");
    }

    click(cx, "crumb-more");
    assert_eq!(menu_names(&view, cx).await, ["a", "b", "c"]);
    assert!(drawn(cx, "path-menu"));
    click(cx, "path-menu-1");
    wait_for_folder(&view, cx, &tree.path().join("a/b")).await;
    menu_closed(&view, cx).await;
}

#[gpui::test]
async fn a_click_on_the_folded_crumb_again_closes_its_menu(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("path");
    let deep = tree.path().join("a/b/c/d/e/f");
    fs::create_dir_all(&deep).unwrap();
    let (view, cx) = path_window(cx, &tree, &deep).await;

    click(cx, "crumb-more");
    menu_names(&view, cx).await;
    click(cx, "crumb-more");
    menu_closed(&view, cx).await;
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
