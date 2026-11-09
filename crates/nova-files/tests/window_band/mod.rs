//! Dragging a box over the entries on a real core: it selects the rows it
//! touches, Ctrl adds them, a press on a name drags no box, and a click on
//! the empty space below the rows selects nothing.

use gpui::{
    Bounds, MouseButton, Pixels, Point, ScrollDelta, ScrollWheelEvent, TouchPhase, point, px,
};

use super::*;

/// The shared tree plus `a.txt`, `b.txt` and `c.txt`:
/// Projects, a.txt, b.txt, c.txt, notes.txt.
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

/// Presses at `from`, moves to `to` and lets go there, with `modifiers`.
fn drag(cx: &mut VisualTestContext, from: Point<Pixels>, to: Point<Pixels>, modifiers: Modifiers) {
    cx.simulate_mouse_down(from, MouseButton::Left, modifiers);
    cx.simulate_mouse_move(to, Some(MouseButton::Left), modifiers);
    cx.simulate_mouse_up(to, MouseButton::Left, modifiers);
    cx.run_until_parked();
}

/// Drags from `from` to `to`, checking the box is drawn from the press to
/// the mouse while it's held and gone once let go.
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

    // A box along one row selects it, and letting go on it isn't a click.
    drag(cx, from, from + point(px(30.0), px(0.0)), Modifiers::none());
    assert_eq!(selected(&view, cx), ["a.txt"]);

    // Past the last row it stops at the last row.
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
    // Letting go on the row a box started on isn't a Ctrl+click on it.
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
    // A move with the button up (let go outside the window) ends it.
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
    assert_eq!(selected(&view, cx), Vec::<String>::new());
    // The next press, off the name, can.
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
    // Ctrl keeps what is selected.
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
    cx.simulate_event(ScrollWheelEvent {
        position: list,
        delta: ScrollDelta::Pixels(point(px(0.0), px(-320.0))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    cx.run_until_parked();
    assert_eq!(cx.debug_bounds("entry-1"), None, "the list scrolled");

    let from = bounds(cx, "modified-15").center();
    let to = bounds(cx, "modified-17").center() + point(px(40.0), px(0.0));
    drag_drawn(cx, from, to);
    // Row 0 is `Projects`, so row 15 is `f14`.
    assert_eq!(selected(&view, cx), ["f14", "f15", "f16"]);
}
