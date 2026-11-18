//! Icons against a scripted core: the entries sit in lines of cells with
//! no column heads, the arrows move through the grid, Ctrl+scroll steps
//! the icons' size, thumbnails are asked for at the size drawn, and a box
//! selects the cells it covers.

use gpui::{Bounds, Modifiers, MouseButton, Pixels, ScrollDelta, ScrollWheelEvent, TouchPhase};
use nova_files_api::IconSize;

use super::*;

/// Enough entries for a few lines of icons.
/// Prime, so the last line is short whatever fits on one.
const CELLS: u32 = 61;

/// A window on [`SHOWN`] showing [`CELLS`] entries as Icons.
fn icons(cx: &mut TestAppContext) -> (Entity<MainWindow>, &mut VisualTestContext, Core) {
    cx.update(nova_files_ui::bind_window_keys);
    let (view, cx, mut core) = window(cx);
    answer_listing(&mut core, CELLS);
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-2");
    cx.run_until_parked();
    core.commands();
    (view, cx, core)
}

fn cell(cx: &mut VisualTestContext, index: u32) -> Bounds<Pixels> {
    cx.run_until_parked();
    cx.debug_bounds(Box::leak(format!("entry-{index}").into_boxed_str()))
        .unwrap()
}

/// How many cells the first line holds.
fn per_line(cx: &mut VisualTestContext) -> u32 {
    let top = cell(cx, 0).top();
    (0..CELLS)
        .take_while(|&index| cell(cx, index).top() == top)
        .count()
        .try_into()
        .unwrap()
}

fn cursor(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Option<usize> {
    view.read_with(cx, |view, _| view.cursor())
}

fn selected(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, _| view.selected_names())
}

/// The sizes of thumbnail the window asked for since last asked.
fn sizes_asked(core: &mut Core) -> Vec<ThumbnailSize> {
    core.commands()
        .into_iter()
        .filter_map(|command| match command {
            Command::Rows { size, count, .. } if count > 0 => Some(size),
            _ => None,
        })
        .collect()
}

fn ctrl_scroll(cx: &mut VisualTestContext, lines: f32) {
    let position = cell(cx, 0).center();
    cx.simulate_event(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Lines(point(0.0, lines)),
        modifiers: Modifiers::secondary_key(),
        touch_phase: TouchPhase::Moved,
    });
    cx.run_until_parked();
}

fn icon_size(view: &Entity<MainWindow>, cx: &VisualTestContext) -> IconSize {
    view.read_with(cx, |view, _| view.folder_view().icons)
}

#[gpui::test]
fn icons_sit_in_lines_of_cells_without_column_heads(cx: &mut TestAppContext) {
    let (_, cx, _) = icons(cx);
    assert_eq!(cx.debug_bounds("head-name"), None);
    let across = per_line(cx);
    assert!(across > 1, "{across} to a line");
    let (first, second, below) = (cell(cx, 0), cell(cx, 1), cell(cx, across));
    assert_eq!(second.left(), first.right());
    assert_eq!(below.left(), first.left());
    assert_eq!(below.top(), first.bottom());

    // A narrower window fits fewer to a line.
    let wide = cx.update(|window, _| window.viewport_size());
    cx.simulate_resize(gpui::size(wide.width / 2.0, wide.height));
    // The window draws once on the new size, and again once it knows how
    // wide the list is there.
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let narrow = per_line(cx);
    assert!(narrow < across, "{narrow} of {across}");

    // Details has its heads and a row each.
    cx.simulate_keystrokes("ctrl-1");
    assert!(cx.debug_bounds("head-name").is_some());
    assert_eq!(cell(cx, 1).top(), cell(cx, 0).bottom());
}

#[gpui::test]
fn the_arrows_move_through_the_grid(cx: &mut TestAppContext) {
    let (view, cx, _) = icons(cx);
    let across = usize::try_from(per_line(cx)).unwrap();

    cx.simulate_keystrokes("right");
    assert_eq!(cursor(&view, cx), Some(0));
    cx.simulate_keystrokes("right down");
    assert_eq!(cursor(&view, cx), Some(1 + across));
    cx.simulate_keystrokes("left");
    assert_eq!(cursor(&view, cx), Some(across));
    cx.simulate_keystrokes("up");
    assert_eq!(cursor(&view, cx), Some(0));
    cx.simulate_keystrokes("shift-right");
    assert_eq!(selected(&view, cx), ["0", "1"]);
    cx.simulate_keystrokes("right shift-left");
    assert_eq!(selected(&view, cx), ["1", "2"]);
    cx.simulate_keystrokes("left");

    // Details takes no notice of left and right.
    cx.simulate_keystrokes("ctrl-1 right shift-right shift-left");
    assert_eq!(cursor(&view, cx), Some(0));
    cx.simulate_keystrokes("down");
    assert_eq!(cursor(&view, cx), Some(1));
}

#[gpui::test]
fn ctrl_scroll_steps_the_icons_size(cx: &mut TestAppContext) {
    let (view, cx, _) = icons(cx);
    assert_eq!(icon_size(&view, cx), IconSize::Medium);

    ctrl_scroll(cx, 1.0);
    assert_eq!(icon_size(&view, cx), IconSize::Large);
    ctrl_scroll(cx, -2.0);
    assert_eq!(icon_size(&view, cx), IconSize::Small);
    // Half a step waits for the rest.
    ctrl_scroll(cx, 0.5);
    assert_eq!(icon_size(&view, cx), IconSize::Small);
    ctrl_scroll(cx, 0.5);
    assert_eq!(icon_size(&view, cx), IconSize::Medium);

    // Details has no icons to size.
    cx.simulate_keystrokes("ctrl-1");
    ctrl_scroll(cx, 1.0);
    assert_eq!(icon_size(&view, cx), IconSize::Medium);
}

#[gpui::test]
fn thumbnails_are_asked_for_at_the_size_drawn(cx: &mut TestAppContext) {
    let (_, cx, mut core) = icons(cx);

    // The largest icons are wider than a normal thumbnail.
    cx.simulate_keystrokes("ctrl-= ctrl-=");
    cx.run_until_parked();
    assert_eq!(sizes_asked(&mut core).last(), Some(&ThumbnailSize::Large));
    cx.simulate_keystrokes("ctrl-1");
    cx.run_until_parked();
    assert_eq!(sizes_asked(&mut core).last(), Some(&ThumbnailSize::Normal));
}

#[gpui::test]
fn a_box_selects_the_cells_it_covers(cx: &mut TestAppContext) {
    let (view, cx, _) = icons(cx);
    let across = per_line(cx);
    // From a corner of the first cell to one of the second line's second,
    // clear of their icons and names.
    let inset = point(px(3.0), px(3.0));
    let from = cell(cx, 0).origin + inset;
    let to = cell(cx, across + 1).bottom_right() - inset;
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    let names = |indices: [u32; 4]| indices.map(|index| index.to_string()).to_vec();
    assert_eq!(selected(&view, cx), names([0, 1, across, across + 1]));
    assert_eq!(
        cursor(&view, cx),
        Some(usize::try_from(across + 1).unwrap())
    );

    // A click below the last line selects nothing.
    let after = point(cell(cx, 0).left(), cell(cx, CELLS - 1).bottom()) + point(px(10.0), px(10.0));
    cx.simulate_mouse_down(after, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(after, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(selected(&view, cx), Vec::<String>::new());
}

#[gpui::test]
fn the_bar_switch_shows_details_or_icons(cx: &mut TestAppContext) {
    let (view, cx, _) = icons(cx);
    let mode = |cx: &VisualTestContext| view.read_with(cx, |view, _| view.folder_view().mode);

    let list = cx.debug_bounds("view-list").unwrap().center();
    cx.simulate_click(list, Modifiers::none());
    assert_eq!(mode(cx), ViewMode::Details);
    let tiles = cx.debug_bounds("view-tiles").unwrap().center();
    cx.simulate_click(tiles, Modifiers::none());
    assert_eq!(mode(cx), ViewMode::Icons);
}

#[gpui::test]
fn left_and_right_step_the_quick_look_once(cx: &mut TestAppContext) {
    let (view, cx, _) = icons(cx);
    cx.simulate_keystrokes("right space");
    assert!(view.read_with(cx, |view, _| view.previewing()));
    cx.simulate_keystrokes("right");
    assert_eq!(cursor(&view, cx), Some(1));
    cx.simulate_keystrokes("left");
    assert_eq!(cursor(&view, cx), Some(0));
}

#[gpui::test]
fn the_menu_key_opens_the_menu_under_the_cell(cx: &mut TestAppContext) {
    let (view, cx, _) = icons(cx);
    let across = per_line(cx);
    cx.simulate_keystrokes("right right down");
    cx.simulate_keystrokes("menu");
    let at = view.read_with(cx, |view, _| view.menu().unwrap().at);
    let under = cell(cx, across + 1);
    assert!(
        at.x > under.left() && at.x < under.right(),
        "{at:?} {under:?}"
    );
    assert_eq!(at.y, under.bottom());
}

#[gpui::test]
fn a_click_after_the_last_cell_on_its_line_selects_nothing(cx: &mut TestAppContext) {
    let (view, cx, _) = icons(cx);
    cx.simulate_keystrokes("right");
    let last = cell(cx, CELLS - 1);
    let after = last.center() + point(last.size.width, px(0.0));
    cx.simulate_mouse_down(after, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(after, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(selected(&view, cx), Vec::<String>::new());
}
