//! The sidebar on a real core: a folder pinned in the bookmarks file shows
//! in Places after the home folder and opens when clicked, and a narrow
//! window folds Places into one row. The + on Places opens a folder picker
//! that pins what it picks, and right-click on a pinned folder takes it
//! out.

use gpui::{MouseButton, MouseDownEvent, MouseUpEvent, size};

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

#[gpui::test]
async fn right_click_on_a_pinned_folder_takes_it_out(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("sidebar-unpin");
    let (view, cx) = with_projects_pinned(cx, &tree).await;
    // Home is a standard folder: no menu.
    right_click(cx, "place-0");
    view.read_with(cx, |view, _| assert!(view.menu().is_none()));

    right_click(cx, "place-1");
    view.read_with(cx, |view, _| {
        let menu = view.menu().unwrap();
        assert_eq!(menu.items.len(), 1);
        assert_eq!(menu.items[0].label, "Remove from Places");
    });
    click(cx, "menu-0");

    view.condition::<WindowEvent>(cx, |view, _| view.pinned().is_empty())
        .await;
    view.read_with(cx, |view, _| assert!(view.menu().is_none()));
    let saved = fs::read_to_string(tree.path().join(".config/gtk-3.0/bookmarks")).unwrap();
    assert_eq!(saved, "");
}

#[gpui::test]
async fn the_plus_on_places_pins_the_folder_picked(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("sidebar-pin");
    let (view, cx) = window_on(cx, tree.path(), tree.path());
    wait_for_folder(&view, cx, tree.path()).await;
    let windows = cx.windows().len();

    click(cx, "pin");
    cx.run_until_parked();

    let picker = cx
        .windows()
        .into_iter()
        .filter_map(|handle| handle.downcast::<MainWindow>())
        .find(|handle| {
            handle
                .read_with(cx, |view, _| view.pick_id().is_some())
                .unwrap()
        })
        .unwrap();
    assert_eq!(cx.windows().len(), windows + 1);
    let projects = tree.path().join("Projects");
    picker
        .update(cx, |view, _, _| assert!(view.answer(projects.clone())))
        .unwrap();

    view.condition::<WindowEvent>(cx, |view, _| {
        view.pinned().iter().any(|pin| pin.path == projects)
    })
    .await;
}

/// Right-clicks the element drawn with `selector`.
fn right_click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    let position = cx.debug_bounds(selector).unwrap().center();
    cx.simulate_event(MouseDownEvent {
        position,
        modifiers: Modifiers::none(),
        button: MouseButton::Right,
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position,
        modifiers: Modifiers::none(),
        button: MouseButton::Right,
        click_count: 1,
    });
    cx.run_until_parked();
}

#[gpui::test]
async fn the_tree_opens_in_place_and_follows_the_folder_on_screen(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("sidebar-tree");
    fs::create_dir(tree.path().join(".hidden")).unwrap();
    let (view, cx) = window_on(cx, tree.path(), tree.path());
    wait_for_folder(&view, cx, tree.path()).await;
    cx.run_until_parked();
    // Home, closed: only its own row.
    assert!(cx.debug_bounds("tree-0").is_some());
    assert!(cx.debug_bounds("tree-1").is_none());

    click(cx, "tree-toggle-0");
    cx.run_until_parked();
    // Projects, but not the hidden folder or the file.
    assert!(cx.debug_bounds("tree-1").is_some());
    assert!(cx.debug_bounds("tree-2").is_none());

    // Going into Projects/nova opens the tree down to it.
    view.update(cx, |view, _| view.open(&tree.path().join("Projects/nova")));
    wait_for_folder(&view, cx, &tree.path().join("Projects/nova")).await;
    cx.run_until_parked();
    assert!(cx.debug_bounds("tree-2").is_some());
    // nova holds no folders: no arrow.
    assert!(cx.debug_bounds("tree-toggle-1").is_some());
    assert!(cx.debug_bounds("tree-toggle-2").is_none());

    // Clicking a row goes there; the arrow only opens and closes.
    click(cx, "tree-0");
    wait_for_folder(&view, cx, tree.path()).await;
    click(cx, "tree-toggle-1");
    cx.run_until_parked();
    assert!(cx.debug_bounds("tree-2").is_none());
    view.read_with(cx, |view, _| assert_eq!(view.folder(), Some(tree.path())));
}

#[gpui::test]
async fn the_system_drive_shows_its_free_space_and_opens(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("sidebar-drives");
    let (view, cx) = open_on(cx, tree.path()).await;
    view.condition::<WindowEvent>(cx, |view, _| view.disk_space(Path::new("/")).is_some())
        .await;
    cx.run_until_parked();
    assert!(cx.debug_bounds("drive-0").is_some());

    // The system drive can't be ejected: its menu only opens it.
    right_click(cx, "drive-0");
    view.read_with(cx, |view, _| {
        let labels: Vec<_> = view
            .menu()
            .unwrap()
            .items
            .iter()
            .map(|item| item.label.clone())
            .collect();
        assert_eq!(labels, ["Open"]);
    });
    click(cx, "menu-0");
    wait_for_folder(&view, cx, Path::new("/")).await;
}
