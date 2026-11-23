use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, ElementId, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement as _, Pixels, Point, Render, ShapedLine, SharedString, Styled as _,
    UTF16Selection, Window, div, prelude::*, px,
};

use super::edit::{Step, TextEdit, offset_from_utf16};
use super::element::{TextElement, span};
use super::{
    Backspace, Cancel, Copy, Cut, Delete, DeleteWordLeft, DeleteWordRight, End, Home, KEY_CONTEXT,
    Left, Paste, Right, SelectAll, SelectEnd, SelectHome, SelectLeft, SelectRight, SelectWordLeft,
    SelectWordRight, Submit, WordLeft, WordRight,
};
use crate::theme::{size, text as type_scale};

const DOUBLE_CLICK: usize = 2;
const DOT: char = '•';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextFieldEvent {
    Changed,
    Submitted,
    Cancelled,
}

pub struct TextField {
    focus: FocusHandle,
    pub(super) edit: TextEdit,
    pub(super) placeholder: SharedString,
    pub(super) layout: Option<(ShapedLine, Bounds<Pixels>)>,
    pub(super) scroll: Pixels,
    pub(super) hidden: bool,
    selecting: bool,
}

impl TextField {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            edit: TextEdit::default(),
            placeholder: SharedString::default(),
            layout: None,
            scroll: px(0.0),
            hidden: false,
            selecting: false,
        }
    }

    #[must_use]
    pub const fn hiding(mut self) -> Self {
        self.hidden = true;
        self
    }

    pub(super) fn shown(&self) -> String {
        if self.hidden {
            dots(self.edit.text())
        } else {
            self.edit.text().to_owned()
        }
    }

    pub(super) fn shown_index(&self, offset: usize) -> usize {
        if self.hidden {
            dot_index(self.edit.text(), offset)
        } else {
            offset
        }
    }

    fn text_offset(&self, index: usize) -> usize {
        if self.hidden {
            offset_of_dot(self.edit.text(), index)
        } else {
            index
        }
    }

    #[must_use]
    pub fn with_placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn text(&self) -> &str {
        self.edit.text()
    }

    pub fn selection(&self) -> Range<usize> {
        self.edit.selection()
    }

    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.edit = TextEdit::new(text);
        cx.notify();
    }

    pub fn select_range(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.edit.select_range(range);
        cx.notify();
    }

    fn edit(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut TextEdit) -> bool) {
        if change(&mut self.edit) {
            cx.emit(TextFieldEvent::Changed);
        }
        cx.notify();
    }

    fn select(&mut self, cx: &mut Context<Self>, step: impl FnOnce(&mut TextEdit)) {
        step(&mut self.edit);
        cx.notify();
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        self.select(cx, |edit| edit.move_left(Step::Grapheme, false));
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        self.select(cx, |edit| edit.move_right(Step::Grapheme, false));
    }

    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let hidden = self.hidden;
        self.select(cx, |edit| word_left(edit, hidden, false));
    }

    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        let hidden = self.hidden;
        self.select(cx, |edit| word_right(edit, hidden, false));
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select(cx, |edit| edit.move_left(Step::Grapheme, true));
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select(cx, |edit| edit.move_right(Step::Grapheme, true));
    }

    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let hidden = self.hidden;
        self.select(cx, |edit| word_left(edit, hidden, true));
    }

    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        let hidden = self.hidden;
        self.select(cx, |edit| word_right(edit, hidden, true));
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.select(cx, |edit| edit.move_home(false));
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.select(cx, |edit| edit.move_end(false));
    }

    fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.select(cx, |edit| edit.move_home(true));
    }

    fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select(cx, |edit| edit.move_end(true));
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.select(cx, TextEdit::select_all);
    }

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        self.edit(cx, |edit| edit.delete_backward(Step::Grapheme));
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        self.edit(cx, |edit| edit.delete_forward(Step::Grapheme));
    }

    fn delete_word_left(&mut self, _: &DeleteWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let hidden = self.hidden;
        self.edit(cx, |edit| {
            if hidden {
                edit.move_home(true);
                edit.replace_selection("")
            } else {
                edit.delete_backward(Step::Word)
            }
        });
    }

    fn delete_word_right(&mut self, _: &DeleteWordRight, _: &mut Window, cx: &mut Context<Self>) {
        let hidden = self.hidden;
        self.edit(cx, |edit| {
            if hidden {
                edit.move_end(true);
                edit.replace_selection("")
            } else {
                edit.delete_forward(Step::Word)
            }
        });
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if self.hidden {
            return;
        }
        let selected = self.edit.selected_text();
        if !selected.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(selected.to_owned()));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if self.hidden || self.edit.selection().is_empty() {
            return;
        }
        self.copy(&Copy, window, cx);
        self.edit(cx, |edit| edit.replace_selection(""));
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let text = text
            .trim_end_matches(['\r', '\n'])
            .replace("\r\n", " ")
            .replace(['\r', '\n'], " ");
        self.edit(cx, |edit| edit.replace_selection(&text));
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let offset = self.offset_at(event.position);
        if event.click_count >= DOUBLE_CLICK && self.hidden {
            self.select(cx, TextEdit::select_all);
            return;
        }
        if event.click_count >= DOUBLE_CLICK {
            self.select(cx, |edit| edit.select_word_at(offset));
            return;
        }
        self.selecting = true;
        if event.modifiers.shift {
            self.select(cx, |edit| edit.select_to(offset));
        } else {
            self.select(cx, |edit| edit.select_range(offset..offset));
        }
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selecting {
            return;
        }
        let offset = self.offset_at(event.position);
        self.select(cx, |edit| edit.select_to(offset));
    }

    const fn mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.selecting = false;
    }

    fn offset_at(&self, position: Point<Pixels>) -> usize {
        let Some((line, bounds)) = &self.layout else {
            return self.edit.text().len();
        };
        self.text_offset(line.closest_index_for_x(position.x - bounds.left() + self.scroll))
    }
}

fn word_left(edit: &mut TextEdit, hidden: bool, extend: bool) {
    if hidden {
        edit.move_home(extend);
    } else {
        edit.move_left(Step::Word, extend);
    }
}

fn word_right(edit: &mut TextEdit, hidden: bool, extend: bool) {
    if hidden {
        edit.move_end(extend);
    } else {
        edit.move_right(Step::Word, extend);
    }
}

fn dots(text: &str) -> String {
    DOT.to_string().repeat(text.chars().count())
}

fn dot_index(text: &str, offset: usize) -> usize {
    text.get(..offset).unwrap_or_default().chars().count() * DOT.len_utf8()
}

fn offset_of_dot(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index / DOT.len_utf8())
        .map_or(text.len(), |(at, _)| at)
}

impl EventEmitter<TextFieldEvent> for TextField {}

impl Focusable for TextField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TextField {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id(ElementId::NamedInteger(
                "text-field".into(),
                cx.entity_id().as_u64(),
            ))
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .cursor(CursorStyle::IBeam)
            .w_full()
            .text_size(type_scale::BODY)
            .line_height(size::FIELD_LINE)
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_word_left))
            .on_action(cx.listener(Self::delete_word_right))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(|_, _: &Submit, _, cx| {
                cx.emit(TextFieldEvent::Submitted);
            }))
            .on_action(cx.listener(|_, _: &Cancel, _, cx| {
                cx.emit(TextFieldEvent::Cancelled);
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up))
            .child(TextElement::new(cx.entity()))
    }
}

// GPUI's input handler counts UTF-16 units; the field counts bytes.
// https://github.com/ImperaLuna/zed/tree/610fab89331e30736906b44f33bffe0cedcae37e/crates/gpui
impl EntityInputHandler for TextField {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.edit.range_from_utf16(&range);
        *actual = Some(self.edit.range_to_utf16(&range));
        self.edit.text().get(range).map(str::to_owned)
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.edit.range_to_utf16(&self.edit.selection()),
            reversed: self.edit.caret() != self.edit.selection().end,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.edit
            .marked()
            .map(|range| self.edit.range_to_utf16(&range))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.select(cx, TextEdit::unmark);
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range.map(|range| self.edit.range_from_utf16(&range));
        self.edit(cx, |edit| edit.replace(range, text));
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range.map(|range| self.edit.range_from_utf16(&range));
        let selected = selected.map(|selected| {
            offset_from_utf16(text, selected.start)..offset_from_utf16(text, selected.end)
        });
        self.edit(cx, |edit| {
            edit.replace_and_mark(range, text, selected);
            true
        });
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let (line, _) = self.layout.as_ref()?;
        let range = self.edit.range_from_utf16(&range);
        Some(span(
            bounds,
            self.scroll,
            line.x_for_index(self.shown_index(range.start)),
            line.x_for_index(self.shown_index(range.end)),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let (line, bounds) = self.layout.as_ref()?;
        let index = line.index_for_x(point.x - bounds.left() + self.scroll)?;
        Some(self.edit.offset_to_utf16(self.text_offset(index)))
    }
}

#[cfg(test)]
mod tests;
