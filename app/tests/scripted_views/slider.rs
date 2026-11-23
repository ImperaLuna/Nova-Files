use gpui::{Modifiers, MouseButton, Pixels, Point};
use nova_files_folders_api::IconSize;

use super::icons::{cell, icon_size, icons};
use super::*;

fn centre(cx: &mut VisualTestContext, name: &'static str) -> Option<Point<Pixels>> {
    cx.run_until_parked();
    cx.debug_bounds(name).map(|bounds| bounds.center())
}

fn stop(index: usize) -> &'static str {
    Box::leak(format!("icon-size-{index}").into_boxed_str())
}

fn click(cx: &mut VisualTestContext, at: Point<Pixels>) {
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
}

#[gpui::test]
fn the_icons_button_opens_a_slider_whose_stops_set_the_size(cx: &mut TestAppContext) {
    let (view, cx, _) = icons(cx);
    assert_eq!(centre(cx, "icon-sizes"), None);

    let button = centre(cx, "view-tiles").unwrap();
    click(cx, button);
    assert!(centre(cx, "icon-sizes").is_some());
    for index in 0..IconSize::ALL.len() {
        assert!(centre(cx, stop(index)).is_some(), "stop {index}");
    }

    let last = centre(cx, stop(IconSize::ALL.len() - 1)).unwrap();
    click(cx, last);
    assert_eq!(icon_size(&view, cx), IconSize::Largest);
    let first = centre(cx, stop(0)).unwrap();
    click(cx, first);
    assert_eq!(icon_size(&view, cx), IconSize::Smallest);

    click(cx, button);
    assert_eq!(centre(cx, "icon-sizes"), None);
}

#[gpui::test]
fn dragging_along_the_slider_steps_through_the_sizes(cx: &mut TestAppContext) {
    let (view, cx, _) = icons(cx);
    let button = centre(cx, "view-tiles").unwrap();
    click(cx, button);
    let from = centre(cx, stop(0)).unwrap();
    let to = centre(cx, stop(IconSize::ALL.len() - 2)).unwrap();

    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();

    assert_eq!(icon_size(&view, cx), IconSize::Larger);
}

#[gpui::test]
fn the_slider_closes_on_a_click_elsewhere_and_with_another_view(cx: &mut TestAppContext) {
    let (_, cx, _) = icons(cx);
    let button = centre(cx, "view-tiles").unwrap();
    click(cx, button);
    let elsewhere = cell(cx, 0).center();
    click(cx, elsewhere);
    assert_eq!(centre(cx, "icon-sizes"), None);

    click(cx, button);
    assert!(centre(cx, "icon-sizes").is_some());
    cx.simulate_keystrokes("ctrl-1 ctrl-2");
    assert_eq!(centre(cx, "icon-sizes"), None);
}
