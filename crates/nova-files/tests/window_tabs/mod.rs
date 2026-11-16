//! Tabs on a real core: each tab keeps its own folder, history, selection,
//! order, search and scroll position, and gets them back at once when
//! shown again; the last tab can't be closed; Ctrl+T, Ctrl+W and
//! Ctrl+Tab work them. The Open list draws them at the top of the sidebar,
//! and middle-click opens folders in new tabs.

use gpui::{MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, point};
use nova_files_api::SortKey;

use super::*;

/// Waits until the tab on screen has every answer it asked for.
async fn settled(view: &Entity<MainWindow>, cx: &VisualTestContext) {
    view.condition::<WindowEvent>(cx, |view, _| !view.waiting())
        .await;
    cx.run_until_parked();
}

/// A window on `folder` with its keys bound, the folder on screen.
async fn tabs_on<'a>(
    cx: &'a mut TestAppContext,
    folder: &Path,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        nova_files_ui::bind_window_keys(cx);
        nova_files_ui::bind_text_keys(cx);
    });
    open_on(cx, folder).await
}

fn offset(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Point<Pixels> {
    view.read_with(cx, |view, _| view.scroll().0.borrow().base_handle.offset())
}

#[gpui::test]
async fn a_tab_shown_again_has_its_folder_selection_and_scroll_at_once(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("tabs-back");
    for index in 0..80 {
        fs::write(tree.path().join(format!("file {index:02}")), "").unwrap();
    }
    let (view, cx) = tabs_on(cx, tree.path()).await;
    click(cx, "name-2");
    let scrolled = point(px(0.0), px(-240.0));
    view.read_with(cx, |view, _| {
        view.scroll().0.borrow().base_handle.set_offset(scrolled);
    });

    cx.simulate_keystrokes("ctrl-t");
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.tab_count(), 2);
        assert_eq!(view.current_tab(), 1);
        assert!(view.is_on_start());
        assert_eq!(view.selected_count(), 0);
        assert_eq!(view.back_target(), None);
    });

    view.update(cx, |view, cx| view.switch_tab(0, cx));
    // Before the core answers: the folder, its rows, the selection and
    // the scroll position are already back.
    view.read_with(cx, |view, _| {
        assert_eq!(view.folder(), Some(tree.path()));
        assert_eq!(view.entry_names().len(), 82);
        assert_eq!(view.selected_names(), ["file 01"]);
    });
    assert_eq!(offset(&view, cx), scrolled);

    // Read again in place, and what changed meanwhile comes in.
    fs::write(tree.path().join("new.txt"), "").unwrap();
    view.update(cx, MainWindow::next_tab);
    view.update(cx, MainWindow::next_tab);
    settled(&view, cx).await;
    view.read_with(cx, |view, _| {
        assert_eq!(view.entry_names().len(), 83);
        assert_eq!(view.selected_names(), ["file 01"]);
    });
    assert_eq!(offset(&view, cx), scrolled);
}

#[gpui::test]
async fn each_tab_keeps_its_own_history_and_order(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("tabs-history");
    let projects = tree.path().join("Projects");
    let (view, cx) = tabs_on(cx, tree.path()).await;
    view.update(cx, |view, _| view.sort_by(SortKey::Name));
    settled(&view, cx).await;

    view.update(cx, |view, cx| view.open_in_new_tab(projects.clone(), cx));
    wait_for_folder(&view, cx, &projects).await;
    view.read_with(cx, |view, _| {
        assert_eq!(view.back_target(), None);
        assert!(!view.shown_order().descending);
    });
    view.update(cx, |view, _| view.open(&projects.join("nova")));
    wait_for_folder(&view, cx, &projects.join("nova")).await;

    view.update(cx, MainWindow::previous_tab);
    settled(&view, cx).await;
    view.read_with(cx, |view, _| {
        assert_eq!(view.folder(), Some(tree.path()));
        assert_eq!(view.back_target(), None);
        assert!(view.shown_order().descending);
    });

    view.update(cx, |view, cx| view.switch_tab(1, cx));
    settled(&view, cx).await;
    view.update(cx, MainWindow::go_back);
    wait_for_folder(&view, cx, &projects).await;
}

#[gpui::test]
async fn a_folder_still_on_its_way_is_asked_for_again(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("tabs-on-its-way");
    let projects = tree.path().join("Projects");
    let (view, cx) = tabs_on(cx, tree.path()).await;

    // Left before its answer can come: the answer is dropped.
    view.update(cx, |view, cx| {
        view.open(&projects);
        view.new_tab(&Location::Start, cx);
        assert_eq!(
            view.tab_places(),
            [
                Some(Location::Folder(projects.clone())),
                Some(Location::Start)
            ]
        );
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert!(view.is_on_start()));

    view.update(cx, |view, cx| view.switch_tab(0, cx));
    wait_for_folder(&view, cx, &projects).await;
    view.read_with(cx, |view, _| {
        assert_eq!(view.back_target(), Some(&Location::from(tree.path())));
    });
}

#[gpui::test]
async fn a_search_stays_with_its_tab(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("tabs-search");
    let (view, cx) = tabs_on(cx, tree.path()).await;
    cx.simulate_keystrokes("ctrl-f");
    cx.run_until_parked();
    cx.simulate_input("notes");
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.searching()));

    cx.simulate_keystrokes("ctrl-t");
    cx.run_until_parked();
    view.read_with(cx, |view, cx| {
        assert!(!view.searching());
        assert_eq!(view.query(cx), "");
    });

    cx.simulate_keystrokes("ctrl-tab");
    cx.run_until_parked();
    view.read_with(cx, |view, cx| {
        assert!(view.searching());
        assert_eq!(view.query(cx), "notes");
        assert_eq!(view.folder(), Some(tree.path()));
    });
}

#[gpui::test]
async fn the_last_tab_stays_open(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("tabs-close");
    let projects = tree.path().join("Projects");
    let (view, cx) = tabs_on(cx, tree.path()).await;
    cx.simulate_keystrokes("ctrl-w");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.tab_count()), 1);

    view.update(cx, |view, cx| {
        view.open_in_new_tab(projects.clone(), cx);
        view.new_tab(&Location::Start, cx);
    });
    cx.run_until_parked();
    // Closing a tab in the background leaves the one on screen.
    view.update(cx, |view, cx| view.close_tab(0, cx));
    view.read_with(cx, |view, _| {
        assert_eq!(view.current_tab(), 1);
        assert!(view.is_on_start());
    });
    // Closing the last one in the list shows the one before it.
    cx.simulate_keystrokes("ctrl-w");
    wait_for_folder(&view, cx, &projects).await;
    view.read_with(cx, |view, _| {
        assert_eq!(view.tab_count(), 1);
        assert_eq!(view.current_tab(), 0);
    });
    cx.simulate_keystrokes("ctrl-w");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.tab_count()), 1);
}

/// Middle-clicks the element drawn with `selector`.
fn middle_click(cx: &mut VisualTestContext, selector: &'static str) {
    aux_click(cx, selector, MouseButton::Middle);
}

/// Clicks the element drawn with `selector` with `button`.
fn aux_click(cx: &mut VisualTestContext, selector: &'static str, button: MouseButton) {
    cx.run_until_parked();
    let position = cx.debug_bounds(selector).unwrap().center();
    cx.simulate_event(MouseDownEvent {
        position,
        modifiers: Modifiers::none(),
        button,
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position,
        modifiers: Modifiers::none(),
        button,
        click_count: 1,
    });
    cx.run_until_parked();
}

#[gpui::test]
async fn the_open_list_shows_each_tab_and_works_them(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("tabs-open-list");
    let projects = tree.path().join("Projects");
    let (view, cx) = tabs_on(cx, tree.path()).await;
    cx.run_until_parked();
    // One tab: the + is there, the x isn't.
    assert!(cx.debug_bounds("tab-0").is_some());
    assert!(cx.debug_bounds("tab-0-here").is_some());
    assert!(cx.debug_bounds("tab-close-0").is_none());
    let (open, home) = (
        cx.debug_bounds("open").unwrap(),
        cx.debug_bounds("place-0").unwrap(),
    );
    assert!(open.bottom() <= home.top(), "Open comes before Places");

    click(cx, "new-tab");
    view.read_with(cx, |view, _| {
        assert_eq!(view.tab_count(), 2);
        assert!(view.is_on_start());
    });
    assert!(cx.debug_bounds("tab-1-here").is_some());
    assert!(cx.debug_bounds("tab-0-here").is_none());
    assert!(cx.debug_bounds("tab-close-1").is_some());
    // The x sits a short way in from the row's right end.
    let (row, close) = (
        cx.debug_bounds("tab-1").unwrap(),
        cx.debug_bounds("tab-close-1").unwrap(),
    );
    assert_eq!(row.right() - close.right(), px(6.0));

    click(cx, "tab-0");
    wait_for_folder(&view, cx, tree.path()).await;
    assert_eq!(view.read_with(cx, |view, _| view.current_tab()), 0);

    // Middle-click on a folder opens it in a new tab.
    middle_click(cx, "entry-0");
    wait_for_folder(&view, cx, &projects).await;
    view.read_with(cx, |view, _| {
        assert_eq!(view.tab_count(), 3);
        assert_eq!(view.current_tab(), 2);
    });
    // On a file, nothing.
    click(cx, "tab-0");
    wait_for_folder(&view, cx, tree.path()).await;
    middle_click(cx, "entry-1");
    assert_eq!(view.read_with(cx, |view, _| view.tab_count()), 3);
    // Only the middle button opens a tab: not a right-click on a folder.
    aux_click(cx, "entry-0", MouseButton::Right);
    assert_eq!(view.read_with(cx, |view, _| view.tab_count()), 3);

    // Middle-click on a tab closes it, and so does its x.
    middle_click(cx, "tab-1");
    assert_eq!(view.read_with(cx, |view, _| view.tab_count()), 2);
    click(cx, "tab-close-0");
    wait_for_folder(&view, cx, &projects).await;
    view.read_with(cx, |view, _| assert_eq!(view.tab_count(), 1));
    cx.run_until_parked();
    assert!(cx.debug_bounds("tab-close-0").is_none());

    // Middle-click on a place opens it in a new tab.
    middle_click(cx, "place-0");
    wait_for_folder(&view, cx, tree.path()).await;
    assert_eq!(view.read_with(cx, |view, _| view.tab_count()), 2);
    middle_click(cx, "place-start");
    view.read_with(cx, |view, _| {
        assert_eq!(view.tab_count(), 3);
        assert!(view.is_on_start());
    });
}

#[gpui::test]
async fn a_tab_whose_folder_is_gone_goes_to_start(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("tabs-gone");
    let projects = tree.path().join("Projects");
    let (view, cx) = tabs_on(cx, tree.path()).await;
    view.update(cx, |view, cx| view.open_in_new_tab(projects.clone(), cx));
    wait_for_folder(&view, cx, &projects).await;
    view.update(cx, |view, cx| view.switch_tab(0, cx));
    settled(&view, cx).await;

    fs::remove_dir_all(&projects).unwrap();
    view.update(cx, |view, cx| view.switch_tab(1, cx));
    view.condition::<WindowEvent>(cx, |view, _| view.is_on_start())
        .await;

    // A new tab on a folder that isn't there waits on Start.
    view.update(cx, |view, cx| view.open_in_new_tab(projects.clone(), cx));
    view.condition::<WindowEvent>(cx, |view, _| view.is_on_start() && !view.waiting())
        .await;
    view.read_with(cx, |view, _| assert_eq!(view.tab_count(), 3));
}
