use gpui::{
    Bounds, Modifiers, MouseButton, Pixels, Point, ScrollDelta, ScrollWheelEvent, TouchPhase,
};
use nova_files_folders_api::IconSize;

use super::*;

const CELLS: u32 = 61;

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
    cx.simulate_mouse_move(position, None, Modifiers::none());
    cx.simulate_event(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Lines(point(0.0, lines)),
        modifiers: Modifiers::secondary_key(),
        touch_phase: TouchPhase::Moved,
    });
    cx.run_until_parked();
}

fn icon_size(view: &Entity<MainWindow>, cx: &VisualTestContext) -> IconSize {
    view.read_with(cx, |view, cx| view.folder_view(cx).icons)
}

fn drag_box(cx: &mut VisualTestContext, from: Point<Pixels>, to: Point<Pixels>) {
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
}

fn first_two_lines_two_across(across: u32) -> Vec<String> {
    [0, 1, across, across + 1]
        .map(|index| index.to_string())
        .to_vec()
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

    let wide = cx.update(|window, _| window.viewport_size());
    cx.simulate_resize(gpui::size(wide.width / 2.0, wide.height));
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let narrow = per_line(cx);
    assert!(narrow < across, "{narrow} of {across}");

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
    ctrl_scroll(cx, 0.5);
    assert_eq!(icon_size(&view, cx), IconSize::Small);
    ctrl_scroll(cx, 0.5);
    assert_eq!(icon_size(&view, cx), IconSize::Medium);

    cx.simulate_keystrokes("ctrl-1");
    ctrl_scroll(cx, 1.0);
    assert_eq!(icon_size(&view, cx), IconSize::Medium);
}

#[gpui::test]
fn thumbnails_are_asked_for_at_the_size_drawn(cx: &mut TestAppContext) {
    let (_, cx, mut core) = icons(cx);

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
    let inset = point(px(3.0), px(3.0));
    let from = cell(cx, 0).origin + inset;
    let to = cell(cx, across + 1).bottom_right() - inset;
    drag_box(cx, from, to);
    assert_eq!(selected(&view, cx), first_two_lines_two_across(across));
    assert_eq!(
        cursor(&view, cx),
        Some(usize::try_from(across + 1).unwrap())
    );

    let after = point(cell(cx, 0).left(), cell(cx, CELLS - 1).bottom()) + point(px(10.0), px(10.0));
    cx.simulate_mouse_down(after, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(after, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(selected(&view, cx), Vec::<String>::new());
}

#[gpui::test]
fn the_bar_switch_shows_details_or_icons(cx: &mut TestAppContext) {
    let (view, cx, _) = icons(cx);
    let mode = |cx: &VisualTestContext| view.read_with(cx, |view, cx| view.folder_view(cx).mode);

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
    assert!(view.read_with(cx, previewing));
    cx.simulate_keystrokes("right");
    assert_eq!(cursor(&view, cx), Some(1));
    cx.simulate_keystrokes("left");
    assert_eq!(cursor(&view, cx), Some(0));
}

#[gpui::test]
fn the_menu_key_opens_the_menu_under_the_cell(cx: &mut TestAppContext) {
    let (view, cx, _) = icons(cx);
    let across = per_line(cx);
    cx.simulate_keystrokes("right right right down");
    cx.simulate_keystrokes("menu");
    let at = view.read_with(cx, |view, _| view.menu().unwrap().at);
    let under = cell(cx, across + 2);
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

#[gpui::test]
fn the_cursor_stays_in_view_when_the_view_changes(cx: &mut TestAppContext) {
    let (view, cx, _) = icons(cx);
    cx.simulate_keystrokes("end");
    assert_eq!(cursor(&view, cx), Some(usize::try_from(CELLS - 1).unwrap()));
    cx.simulate_keystrokes("ctrl-1");
    cx.run_until_parked();
    assert!(cx.debug_bounds("entry-60").is_some());
}

#[gpui::test]
fn a_box_dragged_down_and_left_selects_the_cells_between(cx: &mut TestAppContext) {
    let (view, cx, _) = icons(cx);
    let across = per_line(cx);
    let inset = point(px(3.0), px(3.0));
    let from = point(cell(cx, 1).right(), cell(cx, 1).top()) + point(-inset.x, inset.y);
    let to = point(cell(cx, across).left(), cell(cx, across).bottom()) + point(inset.x, -inset.y);
    drag_box(cx, from, to);
    assert_eq!(selected(&view, cx), first_two_lines_two_across(across));
    assert_eq!(cursor(&view, cx), Some(usize::try_from(across).unwrap()));
}

#[gpui::test]
fn a_rename_under_an_icon_takes_the_cells_width(cx: &mut TestAppContext) {
    let (view, cx, _) = icons(cx);
    cx.simulate_keystrokes("right f2");
    cx.run_until_parked();
    assert!(view.read_with(cx, MainWindow::renaming));
    let field = cx.debug_bounds("rename").unwrap();
    let under = cell(cx, 0);
    assert!(
        field.size.width > under.size.width / 2.0,
        "{field:?} {under:?}"
    );
}

#[gpui::test]
fn ctrl_scroll_takes_whole_steps_at_once(cx: &mut TestAppContext) {
    let (view, cx, _) = icons(cx);
    ctrl_scroll(cx, 2.5);
    assert_eq!(icon_size(&view, cx), IconSize::Largest);
    ctrl_scroll(cx, -3.5);
    assert_eq!(icon_size(&view, cx), IconSize::Small);
}

#[gpui::test]
fn only_ctrl_scroll_over_the_entries_steps_the_icons(cx: &mut TestAppContext) {
    let (view, cx, _) = icons(cx);
    let position = cell(cx, 0).center();
    cx.simulate_mouse_move(position, None, Modifiers::none());
    cx.simulate_event(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Lines(point(0.0, 1.0)),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    cx.run_until_parked();
    assert_eq!(icon_size(&view, cx), IconSize::Medium);

    let outside = point(px(1.0), px(1.0));
    cx.simulate_mouse_move(outside, None, Modifiers::none());
    cx.simulate_event(ScrollWheelEvent {
        position: outside,
        delta: ScrollDelta::Lines(point(0.0, 1.0)),
        modifiers: Modifiers::secondary_key(),
        touch_phase: TouchPhase::Moved,
    });
    cx.run_until_parked();
    assert_eq!(icon_size(&view, cx), IconSize::Medium);
}

#[gpui::test]
fn ctrl_scroll_does_not_scroll_the_entries(cx: &mut TestAppContext) {
    cx.update(nova_files_ui::bind_window_keys);
    let (view, cx, mut core) = window(cx);
    answer_listing(&mut core, 400);
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-2 ctrl-- ctrl--");
    cx.run_until_parked();
    let top = cell(cx, 0).top();
    ctrl_scroll(cx, -1.0);
    assert_eq!(icon_size(&view, cx), IconSize::Smallest);
    assert_eq!(cell(cx, 0).top(), top);
}

#[gpui::test]
fn the_cursor_ring_is_on_the_cursors_cell(cx: &mut TestAppContext) {
    let (_, cx, _) = icons(cx);
    cx.simulate_keystrokes("right right");
    cx.run_until_parked();
    let ring = cx.debug_bounds("cursor").unwrap();
    assert!(cell(cx, 1).contains(&ring.center()), "{ring:?}");
}

#[gpui::test]
fn an_icon_beside_its_name_has_the_same_room_above_as_before(cx: &mut TestAppContext) {
    let (_, cx, _) = icons(cx);
    cx.simulate_keystrokes("ctrl-- ctrl--");
    cx.run_until_parked();
    let (icon, around) = (cx.debug_bounds("icon-0").unwrap(), cell(cx, 0));
    assert_eq!(icon.left() - around.left(), icon.top() - around.top());
}
