//! Where the text field puts its text: mouse clicks and drags land on the
//! character under them, and the input method's caret box and hit tests
//! follow the line, scrolled or not. The test text system gives every
//! character the same width. Points are given from the field's left edge.

use gpui::{
    AppContext as _, Bounds, Context, Entity, EntityInputHandler as _, Focusable as _, IntoElement,
    Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _,
    Pixels, Render, Styled as _, TestAppContext, VisualTestContext, Window, div, point, px, size,
};
use nova_files_ui::TextField;

/// A field `width` wide, [`INSET`] in from the window's left.
struct Frame {
    field: Entity<TextField>,
    width: Pixels,
}

/// How far the field sits from the window's left edge.
const INSET: Pixels = px(30.0);

impl Render for Frame {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .pl(INSET)
            .child(div().w(self.width).child(self.field.clone()))
    }
}

/// A focused field `width` wide holding `text`.
fn field<'a>(
    cx: &'a mut TestAppContext,
    width: Pixels,
    text: &str,
) -> (Entity<TextField>, &'a mut VisualTestContext) {
    cx.update(nova_files_ui::bind_text_keys);
    let (frame, cx) = cx.add_window_view(|_, cx| Frame {
        field: cx.new(TextField::new),
        width,
    });
    let field = frame.read_with(cx, |frame, _| frame.field.clone());
    cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
    cx.simulate_input(text);
    cx.run_until_parked();
    (field, cx)
}

/// The box an input method gets for UTF-16 `range`.
fn caret_box(
    field: &Entity<TextField>,
    cx: &mut VisualTestContext,
    range: std::ops::Range<usize>,
) -> Option<Bounds<Pixels>> {
    let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(100.0), px(20.0)));
    field.update_in(cx, |field, window, cx| {
        field.bounds_for_range(range, bounds, window, cx)
    })
}

/// One character's width.
fn char_width(field: &Entity<TextField>, cx: &mut VisualTestContext) -> Pixels {
    char_width_at(field, cx, 0)
}

/// The width of the character at `index`.
fn char_width_at(field: &Entity<TextField>, cx: &mut VisualTestContext, index: usize) -> Pixels {
    caret_box(field, cx, index..index + 1).map_or(px(0.0), |b| b.size.width)
}

/// Characters in a line far wider than its field.
const LONG: usize = 200;
/// The caret's width, as the field draws it.
const CARET_WIDTH: Pixels = px(1.5);

fn index_at(field: &Entity<TextField>, cx: &mut VisualTestContext, x: Pixels) -> Option<usize> {
    field.update_in(cx, |field, window, cx| {
        field.character_index_for_point(point(INSET + x, px(5.0)), window, cx)
    })
}

fn down(cx: &mut VisualTestContext, x: Pixels, modifiers: Modifiers) {
    cx.simulate_event(MouseDownEvent {
        position: point(INSET + x, px(5.0)),
        modifiers,
        button: MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    });
}

fn drag_to(cx: &mut VisualTestContext, x: Pixels) {
    cx.simulate_event(MouseMoveEvent {
        position: point(INSET + x, px(5.0)),
        modifiers: Modifiers::none(),
        pressed_button: Some(MouseButton::Left),
    });
}

fn up(cx: &mut VisualTestContext, x: Pixels) {
    cx.simulate_event(MouseUpEvent {
        position: point(INSET + x, px(5.0)),
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 1,
    });
}

fn selection(field: &Entity<TextField>, cx: &VisualTestContext) -> std::ops::Range<usize> {
    field.read_with(cx, |field, _| field.selection())
}

#[gpui::test]
fn the_input_method_finds_characters_on_the_line(cx: &mut TestAppContext) {
    let (field, cx) = field(cx, px(400.0), "abcdef");
    let w = char_width(&field, cx);
    assert!(w > px(0.0));
    assert_eq!(
        caret_box(&field, cx, 2..4),
        Some(Bounds::from_corners(
            point(w * 2.0, px(0.0)),
            point(w * 4.0, px(20.0))
        ))
    );
    assert_eq!(index_at(&field, cx, w * 3.5), Some(3));
    assert_eq!(index_at(&field, cx, w * 5.5), Some(5));
}

#[gpui::test]
fn an_empty_field_has_no_characters_to_find(cx: &mut TestAppContext) {
    let (field, cx) = field(cx, px(400.0), "");
    assert_eq!(caret_box(&field, cx, 0..0), None);
    assert_eq!(index_at(&field, cx, px(1.0)), None);
}

#[gpui::test]
fn a_drag_selects_until_the_button_comes_up(cx: &mut TestAppContext) {
    let (field, cx) = field(cx, px(400.0), "abcdef");
    let w = char_width(&field, cx);
    down(cx, w * 1.2, Modifiers::none());
    assert_eq!(selection(&field, cx), 1..1);
    drag_to(cx, w * 4.2);
    assert_eq!(selection(&field, cx), 1..4);
    up(cx, w * 4.2);
    drag_to(cx, w * 5.2);
    assert_eq!(selection(&field, cx), 1..4);
}

#[gpui::test]
fn a_shift_click_selects_from_the_caret(cx: &mut TestAppContext) {
    let (field, cx) = field(cx, px(400.0), "abcdef");
    let w = char_width(&field, cx);
    cx.simulate_keystrokes("home right");
    down(cx, w * 3.2, Modifiers::shift());
    up(cx, w * 3.2);
    assert_eq!(selection(&field, cx), 1..3);
}

#[gpui::test]
fn a_long_line_scrolls_to_keep_the_caret_in_view(cx: &mut TestAppContext) {
    let width = px(100.0);
    let (field, cx) = field(cx, width, &"a".repeat(LONG));
    let w = char_width_at(&field, cx, LONG - 1);
    let line = caret_box(&field, cx, 0..LONG).map_or(px(0.0), |b| b.size.width);
    assert!(line > width * 2.0, "{line:?}");
    // The caret at the end sits at the field's right edge.
    let end = caret_box(&field, cx, LONG..LONG).map(|b| b.origin.x);
    let edge = width - CARET_WIDTH;
    assert_eq!(end, Some(edge));
    // Points near either edge find what has scrolled there.
    assert_eq!(index_at(&field, cx, edge - w * 0.2), Some(LONG - 1));
    let x = w * 0.2;
    let first = index_at(&field, cx, x).unwrap_or(0);
    assert!(first > 0);
    let under = caret_box(&field, cx, first..first + 1);
    assert!(
        under.is_some_and(|b| b.left() <= x && x < b.right()),
        "{under:?}"
    );
    down(cx, x, Modifiers::none());
    up(cx, x);
    assert_eq!(selection(&field, cx), first..first);

    // Shorter text scrolls back to the start.
    field.update(cx, |field, cx| field.set_text("ab", cx));
    cx.run_until_parked();
    assert_eq!(
        caret_box(&field, cx, 0..0).map(|b| b.origin.x),
        Some(px(0.0))
    );
}
