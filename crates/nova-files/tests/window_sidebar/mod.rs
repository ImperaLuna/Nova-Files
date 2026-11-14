//! The sidebar on a real core: a folder pinned in the bookmarks file shows
//! in Places after the home folder and opens when clicked, and a narrow
//! window folds Places into one row.

use gpui::size;

use super::*;

/// A window whose home is `tree`, with `Projects` pinned; waits until the
/// pinned folder is listed.
async fn with_projects_pinned<'a>(
    cx: &'a mut TestAppContext,
    tree: &TempFolder,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    let bookmarks = tree.path().join(".config/gtk-3.0");
    fs::create_dir_all(&bookmarks).unwrap();
    let projects = tree.path().join("Projects");
    fs::write(
        bookmarks.join("bookmarks"),
        format!("file://{}\n", projects.display()),
    )
    .unwrap();
    let (view, cx) = window_on(cx, tree.path(), tree.path());
    wait_for_folder(&view, cx, tree.path()).await;
    view.condition::<WindowEvent>(cx, |view, _| !view.pinned().is_empty())
        .await;
    (view, cx)
}

#[gpui::test]
async fn a_pinned_folder_shows_after_home_and_opens(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("sidebar-pinned");
    let (view, cx) = with_projects_pinned(cx, &tree).await;

    click(cx, "place-1");

    wait_for_folder(&view, cx, &tree.path().join("Projects")).await;
}

#[gpui::test]
async fn a_narrow_window_folds_places_into_one_row(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("sidebar-narrow");
    let (_view, cx) = with_projects_pinned(cx, &tree).await;
    cx.simulate_resize(size(px(1000.0), px(600.0)));
    cx.run_until_parked();
    let (home, pinned) = (
        cx.debug_bounds("place-0").unwrap(),
        cx.debug_bounds("place-1").unwrap(),
    );
    assert!(pinned.top() >= home.bottom(), "wide: one under the other");
    let sheet_left = cx.debug_bounds("modified-header").unwrap().left();
    assert!(home.right() < sheet_left, "wide: beside the sheet");

    cx.simulate_resize(size(px(600.0), px(600.0)));
    cx.run_until_parked();
    let (home, pinned) = (
        cx.debug_bounds("place-0").unwrap(),
        cx.debug_bounds("place-1").unwrap(),
    );
    assert_eq!(home.top(), pinned.top(), "narrow: side by side");
    assert!(pinned.left() >= home.right());
    let header = cx.debug_bounds("modified-header").unwrap();
    assert!(header.top() >= home.bottom(), "narrow: above the sheet");
}
