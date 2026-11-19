use gpui::{MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, point, size};
use nova_files_api::SortKey;
use nova_files_window::HOVER_OPEN_AFTER;

use super::*;

async fn settled(view: &Entity<MainWindow>, cx: &VisualTestContext) {
    view.condition::<WindowEvent>(cx, |view, _| !view.waiting())
        .await;
    cx.run_until_parked();
}

async fn tabs_on<'a>(
    cx: &'a mut TestAppContext,
    folder: &Path,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        nova_files_window::bind_window_keys(cx);
        nova_files_window::bind_text_keys(cx);
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
    view.read_with(cx, |view, _| {
        assert_eq!(view.folder(), Some(tree.path()));
        assert_eq!(view.entry_names().len(), 82);
        assert_eq!(view.selected_names(), ["file 01"]);
    });
    assert_eq!(offset(&view, cx), scrolled);

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

    view.update(cx, |view, cx| {
        view.open(&projects);
        view.new_tab(&Location::start(), cx);
        assert_eq!(
            view.tab_places(),
            [
                Some(Location::Folder(projects.clone())),
                Some(Location::start())
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
        assert_eq!(search_query(view, cx), "");
    });

    cx.simulate_keystrokes("ctrl-tab");
    cx.run_until_parked();
    view.read_with(cx, |view, cx| {
        assert!(view.searching());
        assert_eq!(search_query(view, cx), "notes");
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
        view.new_tab(&Location::start(), cx);
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-shift-tab");
    wait_for_folder(&view, cx, &projects).await;
    assert_eq!(view.read_with(cx, |view, _| view.current_tab()), 1);
    cx.simulate_keystrokes("ctrl-tab");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.current_tab()), 2);
    view.update(cx, |view, cx| view.switch_tab(3, cx));
    assert_eq!(view.read_with(cx, |view, _| view.current_tab()), 2);
    view.update(cx, |view, cx| view.close_tab(0, cx));
    view.read_with(cx, |view, _| {
        assert_eq!(view.current_tab(), 1);
        assert!(view.is_on_start());
    });
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

fn middle_click(cx: &mut VisualTestContext, selector: &'static str) {
    aux_click(cx, selector, MouseButton::Middle);
}

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
    let (row, close) = (
        cx.debug_bounds("tab-1").unwrap(),
        cx.debug_bounds("tab-close-1").unwrap(),
    );
    assert_eq!(row.right() - close.right(), px(6.0));

    click(cx, "tab-0");
    wait_for_folder(&view, cx, tree.path()).await;
    assert_eq!(view.read_with(cx, |view, _| view.current_tab()), 0);

    middle_click(cx, "entry-0");
    wait_for_folder(&view, cx, &projects).await;
    view.read_with(cx, |view, cx| {
        assert_eq!(view.tab_count(), 3);
        assert_eq!(view.current_tab(), 2);
        assert_eq!(view.tab_names(cx)[1..], ["Start", "Projects"]);
    });
    click(cx, "tab-0");
    wait_for_folder(&view, cx, tree.path()).await;
    middle_click(cx, "entry-1");
    assert_eq!(view.read_with(cx, |view, _| view.tab_count()), 3);
    aux_click(cx, "entry-0", MouseButton::Right);
    assert_eq!(view.read_with(cx, |view, _| view.tab_count()), 3);

    middle_click(cx, "tab-1");
    assert_eq!(view.read_with(cx, |view, _| view.tab_count()), 2);
    click(cx, "tab-close-0");
    wait_for_folder(&view, cx, &projects).await;
    view.read_with(cx, |view, _| assert_eq!(view.tab_count(), 1));
    cx.run_until_parked();
    assert!(cx.debug_bounds("tab-close-0").is_none());

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

    view.update(cx, |view, cx| view.open_in_new_tab(projects.clone(), cx));
    view.condition::<WindowEvent>(cx, |view, _| view.is_on_start() && !view.waiting())
        .await;
    view.read_with(cx, |view, _| assert_eq!(view.tab_count(), 3));
}

#[gpui::test]
async fn a_tab_dragged_along_the_list_moves_there_and_moves_no_files(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("tabs-reorder");
    let (view, cx) = tabs_on(cx, tree.path()).await;
    let projects = tree.path().join("Projects");
    view.update(cx, |view, cx| {
        view.open_in_new_tab(projects.clone(), cx);
        view.new_tab(&Location::start(), cx);
        view.switch_tab(0, cx);
    });
    settled(&view, cx).await;
    let names = |view: &Entity<MainWindow>, cx: &VisualTestContext| {
        view.read_with(cx, MainWindow::tab_names)
    };
    let before = names(&view, cx);
    let here = before[0].clone();

    let start = cx.debug_bounds("tab-0").unwrap().center();
    let projects_tab = cx.debug_bounds("tab-1").unwrap();
    let below_middle = projects_tab.center() + point(px(0.0), projects_tab.size.height / 4.0);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        start + point(px(0.0), px(6.0)),
        Some(MouseButton::Left),
        Modifiers::none(),
    );
    cx.run_until_parked();
    assert!(cx.debug_bounds("tab-landing").is_none());
    cx.simulate_mouse_move(below_middle, Some(MouseButton::Left), Modifiers::none());
    cx.run_until_parked();
    let line = cx.debug_bounds("tab-landing").unwrap();
    assert!(projects_tab.contains(&line.center()));
    assert!(line.center().y > projects_tab.center().y);
    cx.executor().advance_clock(HOVER_OPEN_AFTER);
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.current_tab()), 0);
    cx.simulate_mouse_up(below_middle, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();

    assert_eq!(
        names(&view, cx),
        [before[1].clone(), here, before[2].clone()]
    );
    view.read_with(cx, |view, _| {
        assert_eq!(view.current_tab(), 1);
        assert_eq!(view.folder(), Some(tree.path()));
        assert_eq!(view.jobs_pending(), 0);
    });
    assert!(cx.debug_bounds("tab-landing").is_none());
    assert!(tree.path().join("notes.txt").exists());
}

fn drag_tab(cx: &mut VisualTestContext, from: &'static str, to: Point<Pixels>) {
    let start = cx.debug_bounds(from).unwrap().center();
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        start + point(px(3.0), px(3.0)),
        Some(MouseButton::Left),
        Modifiers::none(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
    cx.run_until_parked();
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
}

async fn reorder_tabs_by_halves(cx: &mut TestAppContext, test: &str, width: Pixels) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree(test);
    let (view, cx) = tabs_on(cx, tree.path()).await;
    cx.simulate_resize(size(width, px(600.0)));
    view.update(cx, |view, cx| {
        view.open_in_new_tab(tree.path().join("Projects"), cx);
        view.new_tab(&Location::start(), cx);
    });
    settled(&view, cx).await;
    let names = |view: &Entity<MainWindow>, cx: &VisualTestContext| {
        view.read_with(cx, MainWindow::tab_names)
    };
    let [a, b, c] = <[_; 3]>::try_from(names(&view, cx)).unwrap();
    view.update(cx, |view, cx| {
        view.move_tab(3, 0, cx);
        view.move_tab(0, 4, cx);
    });
    assert_eq!(names(&view, cx), [a.clone(), b.clone(), c.clone()]);

    let last = cx.debug_bounds("tab-2").unwrap();
    drag_tab(cx, "tab-0", last.center() + point(px(4.0), px(4.0)));
    assert_eq!(names(&view, cx), [b.clone(), c.clone(), a.clone()]);

    let first = cx.debug_bounds("tab-0").unwrap().center();
    drag_tab(cx, "tab-2", first);
    assert_eq!(names(&view, cx), [a, b, c]);
}

#[gpui::test]
async fn a_tab_lands_by_the_half_of_the_row_it_is_let_go_over(cx: &mut TestAppContext) {
    reorder_tabs_by_halves(cx, "tabs-halves", px(1000.0)).await;
}

#[gpui::test]
async fn in_a_narrow_window_a_tab_lands_by_the_half_across(cx: &mut TestAppContext) {
    reorder_tabs_by_halves(cx, "tabs-halves-narrow", px(600.0)).await;
}
