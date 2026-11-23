use std::time::Duration;

use gpui::{Bounds, MouseButton, Pixels, Point, ScrollDelta, ScrollEvent, TouchPhase, point, px};

use super::*;

async fn lettered<'a>(
    cx: &'a mut TestAppContext,
    tree: &TempFolder,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(tree.path().join(name), "").unwrap();
    }
    let (view, cx) = open_on(cx, tree.path()).await;
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.entry_names(),
            ["Projects", "a.txt", "b.txt", "c.txt", "notes.txt"]
        );
    });
    (view, cx)
}

fn selected(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, _| view.selected_names())
}

fn bounds(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
    cx.run_until_parked();
    cx.debug_bounds(selector).unwrap()
}

fn drag(cx: &mut VisualTestContext, from: Point<Pixels>, to: Point<Pixels>, modifiers: Modifiers) {
    cx.simulate_mouse_down(from, MouseButton::Left, modifiers);
    for _ in 0..2 {
        cx.simulate_mouse_move(to, Some(MouseButton::Left), modifiers);
        cx.run_until_parked();
    }
    cx.simulate_mouse_up(to, MouseButton::Left, modifiers);
    cx.run_until_parked();
}

fn drag_drawn(cx: &mut VisualTestContext, from: Point<Pixels>, to: Point<Pixels>) {
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
    assert_eq!(bounds(cx, "band"), Bounds::from_corners(from, to));
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(cx.debug_bounds("band"), None);
}

#[gpui::test]
async fn a_box_selects_the_rows_it_touches(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("band");
    let (view, cx) = lettered(cx, &tree).await;

    let from = bounds(cx, "modified-1").center();
    let to = bounds(cx, "modified-3").center() + point(px(40.0), px(0.0));
    drag_drawn(cx, from, to);
    assert_eq!(selected(&view, cx), ["a.txt", "b.txt", "c.txt"]);
    assert_eq!(view.read_with(cx, |view, _| view.cursor()), Some(3));

    drag(cx, from, from + point(px(30.0), px(0.0)), Modifiers::none());
    assert_eq!(selected(&view, cx), ["a.txt"]);

    let below = bounds(cx, "entry-4").bottom_left() + point(px(40.0), px(60.0));
    drag(cx, to, below, Modifiers::none());
    assert_eq!(selected(&view, cx), ["c.txt", "notes.txt"]);
}

#[gpui::test]
async fn ctrl_adds_the_box_to_the_selection(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("band");
    let (view, cx) = lettered(cx, &tree).await;

    click(cx, "entry-4");
    let from = bounds(cx, "modified-1").center();
    let to = bounds(cx, "modified-2").center();
    drag(cx, from, to, Modifiers::control());
    assert_eq!(selected(&view, cx), ["a.txt", "b.txt", "notes.txt"]);
    click(cx, "entry-4");
    drag(
        cx,
        from,
        from + point(px(30.0), px(0.0)),
        Modifiers::control(),
    );
    assert_eq!(selected(&view, cx), ["a.txt", "notes.txt"]);
}

#[gpui::test]
async fn shift_adds_the_box_too_and_a_shift_click_on_space_keeps_the_picks(
    cx: &mut TestAppContext,
) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("band-shift");
    let (view, cx) = lettered(cx, &tree).await;

    click(cx, "entry-4");
    let from = bounds(cx, "modified-1").center();
    let to = bounds(cx, "modified-2").center();
    drag(cx, from, to, Modifiers::shift());
    assert_eq!(selected(&view, cx), ["a.txt", "b.txt", "notes.txt"]);
    let below = bounds(cx, "entry-4").bottom_left() + point(px(40.0), px(60.0));
    cx.simulate_click(below, Modifiers::shift());
    assert_eq!(selected(&view, cx), ["a.txt", "b.txt", "notes.txt"]);
}

#[gpui::test]
async fn a_box_starts_only_once_the_mouse_moves_a_few_pixels(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("band");
    let (_view, cx) = lettered(cx, &tree).await;

    let from = bounds(cx, "modified-1").center();
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        from + point(px(3.0), px(-3.0)),
        Some(MouseButton::Left),
        Modifiers::none(),
    );
    cx.run_until_parked();
    assert_eq!(cx.debug_bounds("band"), None);
    cx.simulate_mouse_move(
        from + point(px(-4.0), px(0.0)),
        Some(MouseButton::Left),
        Modifiers::none(),
    );
    cx.run_until_parked();
    assert!(cx.debug_bounds("band").is_some());
    cx.simulate_mouse_move(from, None, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(cx.debug_bounds("band"), None);
}

#[gpui::test]
async fn a_press_on_a_name_drags_no_box(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("band");
    let (view, cx) = lettered(cx, &tree).await;

    let from = bounds(cx, "name-1").center();
    let to = bounds(cx, "modified-3").center();
    drag(cx, from, to, Modifiers::none());
    assert_eq!(selected(&view, cx), ["a.txt"]);
    let from = bounds(cx, "modified-1").center();
    drag(cx, from, to, Modifiers::none());
    assert_eq!(selected(&view, cx), ["a.txt", "b.txt", "c.txt"]);
}

#[gpui::test]
async fn a_click_below_the_rows_selects_nothing(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("band");
    let (view, cx) = lettered(cx, &tree).await;

    click(cx, "entry-2");
    assert_eq!(selected(&view, cx), ["b.txt"]);
    let below = bounds(cx, "entry-4").bottom_left() + point(px(40.0), px(60.0));
    cx.simulate_click(below, Modifiers::control());
    assert_eq!(selected(&view, cx), ["b.txt"]);
    cx.simulate_click(below, Modifiers::none());
    assert_eq!(selected(&view, cx), Vec::<String>::new());
}

#[gpui::test]
async fn a_box_over_empty_space_selects_nothing(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("band");
    let (view, cx) = lettered(cx, &tree).await;

    click(cx, "entry-2");
    let below = bounds(cx, "entry-4").bottom_left() + point(px(40.0), px(60.0));
    drag(
        cx,
        below,
        below + point(px(80.0), px(40.0)),
        Modifiers::control(),
    );
    assert_eq!(selected(&view, cx), ["b.txt"]);
    drag(
        cx,
        below,
        below + point(px(80.0), px(40.0)),
        Modifiers::none(),
    );
    assert_eq!(selected(&view, cx), Vec::<String>::new());
}

#[gpui::test]
async fn a_box_in_a_scrolled_list_selects_the_rows_under_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("band");
    for index in 0..80 {
        fs::write(tree.path().join(format!("f{index:02}")), "").unwrap();
    }
    let (view, cx) = open_on(cx, tree.path()).await;
    let list = bounds(cx, "entry-1").center();
    scroll(cx, list, px(-320.0));
    assert_eq!(cx.debug_bounds("entry-1"), None, "the list scrolled");

    let from = bounds(cx, "modified-15").center();
    let to = bounds(cx, "modified-17").center() + point(px(40.0), px(0.0));
    drag_drawn(cx, from, to);
    assert_eq!(selected(&view, cx), ["f14", "f15", "f16"]);

    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
    let row = bounds(cx, "entry-15").size.height;
    scroll(cx, to, -row * 2.0);
    assert_eq!(selected(&view, cx), ["f14", "f15", "f16", "f17", "f18"]);
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(selected(&view, cx), ["f14", "f15", "f16", "f17", "f18"]);
}

fn scroll(cx: &mut VisualTestContext, position: Point<Pixels>, by: Pixels) {
    cx.simulate_event(ScrollEvent {
        position,
        delta: ScrollDelta::Pixels(point(px(0.0), by)),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
        source: gpui::ScrollSource::Pointer,
    });
    cx.run_until_parked();
}

#[gpui::test]
async fn a_new_folder_ends_the_box(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("band");
    let projects = tree.path().join("Projects");
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(projects.join(name), "").unwrap();
    }
    cx.update(nova_files_window::bind_window_keys);
    let (view, cx) = open_on(cx, &projects).await;
    cx.run_until_parked();

    let from = bounds(cx, "modified-1").center();
    let to = bounds(cx, "modified-2").center() + point(px(40.0), px(0.0));
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_keystrokes("alt-up");
    wait_for_folder(&view, cx, tree.path()).await;
    cx.simulate_mouse_move(from, Some(MouseButton::Left), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(cx.debug_bounds("band"), None);
    assert_eq!(selected(&view, cx), ["Projects"]);
}

#[gpui::test]
async fn a_click_on_a_row_cut_off_by_the_edge_leaves_the_list_where_it_is(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("band-edge");
    for index in 0..80 {
        fs::write(tree.path().join(format!("f{index:02}")), "").unwrap();
    }
    let (view, cx) = open_on(cx, tree.path()).await;
    let row = bounds(cx, "entry-1");
    scroll(cx, row.center(), -row.size.height / 4.0);
    let cut = bounds(cx, "entry-0");
    assert!(
        cut.top() < row.top() - row.size.height,
        "the first row is cut off"
    );

    let at = point(cut.center().x, cut.bottom() - px(4.0));
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(selected(&view, cx), ["Projects"]);
    assert_eq!(bounds(cx, "entry-0"), cut);
}

async fn long_list<'a>(
    cx: &'a mut TestAppContext,
    tree: &TempFolder,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    for index in 0..80 {
        fs::write(tree.path().join(format!("f{index:02}")), "").unwrap();
    }
    open_on(cx, tree.path()).await
}

fn held(cx: &VisualTestContext, wait: Duration) {
    cx.executor().advance_clock(wait);
    cx.run_until_parked();
}

#[gpui::test]
async fn a_box_held_past_the_bottom_keeps_scrolling_and_growing(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("band-edge-hold");
    let (view, cx) = long_list(cx, &tree).await;
    let from = bounds(cx, "modified-1").center();
    let below = point(from.x, bounds(cx, "entry-0").top() + px(2000.0));
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(below, Some(MouseButton::Left), Modifiers::none());
    cx.run_until_parked();
    let before = selected(&view, cx).len();

    held(cx, Duration::from_millis(200));
    assert_eq!(
        cx.debug_bounds("entry-0"),
        None,
        "the list scrolled while held"
    );
    let grown = selected(&view, cx).len();
    assert!(grown > before, "{before} then {grown}");
    held(cx, Duration::from_secs(5));
    assert_eq!(
        selected(&view, cx).last().map(String::as_str),
        Some("notes.txt")
    );

    cx.simulate_mouse_up(below, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    let row = bounds(cx, "entry-81");
    held(cx, Duration::from_millis(500));
    assert_eq!(
        bounds(cx, "entry-81"),
        row,
        "letting go stops the scrolling"
    );
}

#[gpui::test]
async fn files_carried_to_the_bottom_edge_scroll_the_list(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("band-edge-carry");
    let (_view, cx) = long_list(cx, &tree).await;
    let start = bounds(cx, "name-2").center();
    let list_bottom = (0..80)
        .filter_map(|index| cx.debug_bounds(Box::leak(format!("entry-{index}").into_boxed_str())))
        .map(|row| row.bottom())
        .fold(px(0.0), Pixels::max);
    let edge = point(start.x, list_bottom - px(4.0));
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    for at in [
        start + point(px(20.0), px(0.0)),
        edge,
        edge + point(px(1.0), px(0.0)),
    ] {
        cx.simulate_mouse_move(at, Some(MouseButton::Left), Modifiers::none());
        cx.run_until_parked();
    }
    assert!(cx.debug_bounds("drag-chip").is_some());
    let top = bounds(cx, "entry-2");
    held(cx, Duration::from_millis(300));
    assert!(
        cx.debug_bounds("entry-2")
            .is_none_or(|row| row.top() < top.top())
    );
    cx.simulate_mouse_up(edge, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
}

fn list_offset(
    view: &Entity<MainWindow>,
    cx: &VisualTestContext,
) -> (Pixels, Pixels, Bounds<Pixels>) {
    view.read_with(cx, |view, _| {
        let handle = view.scroll().0.borrow().base_handle.clone();
        (-handle.offset().y, handle.max_offset().y, handle.bounds())
    })
}

#[gpui::test]
async fn a_long_folder_shows_a_scroll_bar_while_it_moves_and_hides_it_after(
    cx: &mut TestAppContext,
) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("bar-shows");
    let (view, cx) = long_list(cx, &tree).await;
    cx.run_until_parked();
    let bar = bounds(cx, "scroll-bar");
    let (_, _, list) = list_offset(&view, cx);
    assert!(bar.right() <= list.right() && bar.bottom() < list.bottom() - px(60.0));
    assert!(!view.read_with(cx, |view, _| view.scroll_bar_shown()));

    scroll(cx, list.center(), px(-200.0));
    assert!(view.read_with(cx, |view, _| view.scroll_bar_shown()));
    held(cx, Duration::from_millis(1100));
    assert!(!view.read_with(cx, |view, _| view.scroll_bar_shown()));
}

#[gpui::test]
async fn a_short_folder_shows_no_scroll_bar(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("bar-short");
    let (_view, cx) = lettered(cx, &tree).await;
    assert!(cx.debug_bounds("scroll-bar").is_none());
}

#[gpui::test]
async fn the_thumb_dragged_halfway_shows_the_middle_of_the_folder(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("bar-drag");
    let (view, cx) = long_list(cx, &tree).await;
    cx.run_until_parked();
    let (bar, thumb) = (bounds(cx, "scroll-bar"), bounds(cx, "scroll-thumb"));
    let room = bar.size.height - thumb.size.height;

    let from = thumb.center();
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        from + point(px(0.0), room / 2.0),
        Some(MouseButton::Left),
        Modifiers::none(),
    );
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.scroll_bar_shown()));
    cx.simulate_mouse_up(
        from + point(px(0.0), room / 2.0),
        MouseButton::Left,
        Modifiers::none(),
    );
    cx.run_until_parked();
    let (scrolled, most, _) = list_offset(&view, cx);
    assert!(
        (scrolled - most / 2.0).abs() < px(2.0),
        "{scrolled:?} of {most:?}"
    );
    assert_eq!(selected(&view, cx), Vec::<String>::new());
}

#[gpui::test]
async fn a_click_on_the_bar_below_the_thumb_moves_a_page(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("bar-page");
    let (view, cx) = long_list(cx, &tree).await;
    cx.run_until_parked();
    let (bar, thumb) = (bounds(cx, "scroll-bar"), bounds(cx, "scroll-thumb"));

    cx.simulate_click(
        point(thumb.center().x, bar.bottom() - px(4.0)),
        Modifiers::none(),
    );
    cx.run_until_parked();
    let (scrolled, most, list) = list_offset(&view, cx);
    assert_eq!(scrolled, list.size.height.min(most));
    assert_eq!(selected(&view, cx), Vec::<String>::new());
}
