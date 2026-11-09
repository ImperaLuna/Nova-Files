//! The text field in a window, driven by simulated keys, clicks and an
//! input method: moves and deletes by character and word, selections,
//! the clipboard, Enter and Escape, and composed text.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    ClipboardItem, Entity, EntityInputHandler as _, Focusable as _, Modifiers, MouseButton,
    MouseDownEvent, MouseUpEvent, TestAppContext, VisualTestContext, point, px,
};
use nova_files_ui::{TextField, TextFieldEvent};

/// A focused field filling a window, and the events it has sent.
fn field(
    cx: &mut TestAppContext,
) -> (
    Entity<TextField>,
    &mut VisualTestContext,
    Rc<RefCell<Vec<TextFieldEvent>>>,
) {
    cx.update(nova_files_ui::bind_text_keys);
    let (field, cx) = cx.add_window_view(|_, cx| TextField::new(cx));
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&events);
    cx.update(|window, cx| {
        window.focus(&field.focus_handle(cx), cx);
        cx.subscribe(&field, move |_, event, _| sink.borrow_mut().push(*event))
            .detach();
    });
    cx.run_until_parked();
    (field, cx, events)
}

fn text(field: &Entity<TextField>, cx: &VisualTestContext) -> String {
    field.read_with(cx, |field, _| field.text().to_owned())
}

fn selection(field: &Entity<TextField>, cx: &VisualTestContext) -> std::ops::Range<usize> {
    field.read_with(cx, |field, _| field.selection())
}

#[gpui::test]
fn typing_and_word_deletes(cx: &mut TestAppContext) {
    let (field, cx, events) = field(cx);
    cx.simulate_input("my report-2026.pdf");
    assert_eq!(text(&field, cx), "my report-2026.pdf");

    cx.simulate_keystrokes("ctrl-backspace");
    assert_eq!(text(&field, cx), "my report-2026.");
    cx.simulate_keystrokes("backspace");
    assert_eq!(text(&field, cx), "my report-2026");
    cx.simulate_keystrokes("home ctrl-delete");
    assert_eq!(text(&field, cx), " report-2026");
    cx.simulate_keystrokes("delete");
    assert_eq!(text(&field, cx), "report-2026");
    assert!(
        events
            .borrow()
            .iter()
            .all(|e| *e == TextFieldEvent::Changed)
    );
    assert_eq!(events.borrow().len(), "my report-2026.pdf".len() + 4);
}

#[gpui::test]
fn caret_moves_by_character_word_and_line(cx: &mut TestAppContext) {
    let (field, cx, events) = field(cx);
    cx.simulate_input("one two three");
    events.borrow_mut().clear();

    cx.simulate_keystrokes("ctrl-left");
    assert_eq!(selection(&field, cx), 8..8);
    cx.simulate_keystrokes("left");
    assert_eq!(selection(&field, cx), 7..7);
    cx.simulate_keystrokes("ctrl-right");
    assert_eq!(selection(&field, cx), 13..13);
    cx.simulate_keystrokes("home right");
    assert_eq!(selection(&field, cx), 1..1);
    cx.simulate_keystrokes("end");
    assert_eq!(selection(&field, cx), 13..13);
    // Moving changes no text.
    assert!(events.borrow().is_empty());
}

#[gpui::test]
fn shift_keys_select(cx: &mut TestAppContext) {
    let (field, cx, _) = field(cx);
    cx.simulate_input("one two three");

    cx.simulate_keystrokes("shift-left shift-left");
    assert_eq!(selection(&field, cx), 11..13);
    cx.simulate_keystrokes("ctrl-shift-left");
    assert_eq!(selection(&field, cx), 8..13);
    cx.simulate_keystrokes("shift-home");
    assert_eq!(selection(&field, cx), 0..13);
    cx.simulate_keystrokes("home shift-right");
    assert_eq!(selection(&field, cx), 0..1);
    cx.simulate_keystrokes("ctrl-shift-right");
    assert_eq!(selection(&field, cx), 0..3);
    cx.simulate_keystrokes("shift-end");
    assert_eq!(selection(&field, cx), 0..13);
    cx.simulate_keystrokes("right ctrl-a");
    assert_eq!(selection(&field, cx), 0..13);

    cx.simulate_input("four");
    assert_eq!(text(&field, cx), "four");
}

#[gpui::test]
fn copy_cut_and_paste(cx: &mut TestAppContext) {
    let (field, cx, events) = field(cx);
    cx.simulate_input("notes");
    cx.simulate_keystrokes("ctrl-a ctrl-c end ctrl-v");
    assert_eq!(text(&field, cx), "notesnotes");

    cx.simulate_keystrokes("ctrl-a ctrl-x");
    assert_eq!(text(&field, cx), "");
    let cut = cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(cut.as_deref(), Some("notesnotes"));

    // Nothing selected: copy and cut leave the clipboard alone.
    cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("a\r\nb\nc\r\n".to_owned())));
    cx.simulate_keystrokes("ctrl-c ctrl-x ctrl-v");
    // Each line break is one space; trailing ones go.
    assert_eq!(text(&field, cx), "a b c");
    events.borrow_mut().clear();
    cx.simulate_keystrokes("ctrl-x");
    assert!(events.borrow().is_empty());
}

#[gpui::test]
fn enter_and_escape_go_to_the_owner(cx: &mut TestAppContext) {
    let (field, cx, events) = field(cx);
    cx.simulate_input("x");
    events.borrow_mut().clear();

    cx.simulate_keystrokes("enter escape");
    assert_eq!(
        *events.borrow(),
        [TextFieldEvent::Submitted, TextFieldEvent::Cancelled]
    );
    assert_eq!(text(&field, cx), "x");
}

#[gpui::test]
fn an_input_method_composes_then_commits(cx: &mut TestAppContext) {
    let (field, cx, events) = field(cx);
    cx.simulate_input("a");

    field.update_in(cx, |field, window, cx| {
        field.replace_and_mark_text_in_range(None, "に", None, window, cx);
    });
    assert_eq!(text(&field, cx), "aに");
    let marked = field.update_in(cx, |field, window, cx| field.marked_text_range(window, cx));
    assert_eq!(marked, Some(1..2));

    // The input method counts in UTF-16 within its text.
    field.update_in(cx, |field, window, cx| {
        field.replace_and_mark_text_in_range(None, "にほ", Some(1..2), window, cx);
    });
    assert_eq!(selection(&field, cx), 4..7);

    field.update_in(cx, |field, window, cx| {
        field.replace_text_in_range(None, "日本", window, cx);
    });
    assert_eq!(text(&field, cx), "a日本");
    let marked = field.update_in(cx, |field, window, cx| field.marked_text_range(window, cx));
    assert_eq!(marked, None);
    let selected = field.update_in(cx, |field, window, cx| {
        field.selected_text_range(false, window, cx)
    });
    assert_eq!(selected.map(|selected| selected.range), Some(3..3));
    let mut actual = None;
    let read = field.update_in(cx, |field, window, cx| {
        field.text_for_range(1..3, &mut actual, window, cx)
    });
    assert_eq!((read.as_deref(), actual), (Some("日本"), Some(1..3)));
    assert!(
        events
            .borrow()
            .iter()
            .all(|e| *e == TextFieldEvent::Changed)
    );
}

#[gpui::test]
fn the_owner_sets_text_and_selection(cx: &mut TestAppContext) {
    let (field, cx, events) = field(cx);
    field.update(cx, |field, cx| {
        field.set_text("report.pdf", cx);
        field.select_range(0..6, cx);
    });
    assert_eq!(text(&field, cx), "report.pdf");
    assert_eq!(selection(&field, cx), 0..6);
    // Not a user change.
    assert!(events.borrow().is_empty());

    cx.simulate_input("plan");
    assert_eq!(text(&field, cx), "plan.pdf");
}

#[gpui::test]
fn a_double_click_selects_a_word(cx: &mut TestAppContext) {
    let (field, cx, _) = field(cx);
    cx.simulate_input("my report");
    cx.run_until_parked();

    let position = point(px(1.0), px(5.0));
    for click_count in 1..=2 {
        cx.simulate_event(MouseDownEvent {
            position,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count,
        });
    }
    assert_eq!(selection(&field, cx), 0..2);
}

#[gpui::test]
fn the_input_method_sees_which_end_the_caret_is_at(cx: &mut TestAppContext) {
    let (field, cx, _) = field(cx);
    cx.simulate_input("abc");
    let reversed = |field: &Entity<TextField>, cx: &mut VisualTestContext| {
        field.update_in(cx, |field, window, cx| {
            field
                .selected_text_range(false, window, cx)
                .map(|selected| selected.reversed)
        })
    };
    cx.simulate_keystrokes("shift-left");
    assert_eq!(reversed(&field, cx), Some(true));
    cx.simulate_keystrokes("home shift-right");
    assert_eq!(reversed(&field, cx), Some(false));
}

#[gpui::test]
fn the_input_method_can_stop_composing(cx: &mut TestAppContext) {
    let (field, cx, _) = field(cx);
    field.update_in(cx, |field, window, cx| {
        field.replace_and_mark_text_in_range(None, "に", None, window, cx);
        field.unmark_text(window, cx);
    });
    let marked = field.update_in(cx, |field, window, cx| field.marked_text_range(window, cx));
    assert_eq!(marked, None);
    cx.simulate_input("x");
    assert_eq!(text(&field, cx), "にx");
}
