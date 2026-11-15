//! Draws the island over the bottom of the sheet (see `island`): a note
//! with its Undo, or the selection with what can be done to it. While it
//! is down, the sheet's quiet count shows at the bottom left instead.
//!
//! Its buttons send the window's actions, as keys do (see `commands`);
//! only closing a note, which no key does, is handled here. Each part a
//! test clicks carries a debug selector: `island`, `island-copy`,
//! `island-cut`, `island-trash`, `island-clear`, `island-undo`,
//! `island-dismiss`, and the count, `count`.

use gpui::{
    Action, BoxShadow, ClickEvent, Context, Div, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, Rgba, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _,
    div, point, prelude::FluentBuilder as _, px,
};

use super::row::icon;
use crate::assets::Icon;
use crate::commands::{ClearSelection, CopySelected, CutSelected, TrashSelected, Undo};
use crate::island::{self, Note};
use crate::theme::{Colors, radius, size, text};
use crate::window::MainWindow;

const UNDO_LABEL: &str = "Undo";

impl MainWindow {
    /// The island, or the quiet count while the island is down.
    pub(super) fn island(&self, cx: &Context<Self>) -> impl IntoElement {
        let colors = Colors::of(cx);
        let parts = if self.pick.is_some() {
            None
        } else if let Some(note) = &self.island.note {
            Some(note_parts(note, &colors, cx))
        } else if self.selected_count() > 0 {
            Some(self.selection_parts(&colors))
        } else {
            None
        };
        parts.map_or_else(
            || self.quiet_count(&colors).into_any_element(),
            |parts| floating(parts, &colors).into_any_element(),
        )
    }

    /// "3 selected · 12 MB", then Copy, Cut, Move to Trash and Clear.
    fn selection_parts(&self, colors: &Colors) -> Vec<gpui::AnyElement> {
        let (number, rest) = island::selection_line(self.selected_count(), self.selected_bytes());
        vec![
            label(colors)
                .child(
                    div()
                        .text_color(colors.text)
                        .font_weight(text::STRONG_WEIGHT)
                        .child(number),
                )
                .child(rest)
                .into_any_element(),
            icon_button("island-copy", Icon::Copy, &CopySelected, colors).into_any_element(),
            icon_button("island-cut", Icon::Cut, &CutSelected, colors).into_any_element(),
            icon_button("island-trash", Icon::Trash, &TrashSelected, colors).into_any_element(),
            divider(colors).into_any_element(),
            icon_button("island-clear", Icon::Close, &ClearSelection, colors).into_any_element(),
        ]
    }

    fn quiet_count(&self, colors: &Colors) -> impl IntoElement {
        let line = Some(self.counted.clone()).filter(|line| !line.is_empty());
        div().when_some(line, |slot, line| {
            slot.child(
                div()
                    .debug_selector(|| "count".to_owned())
                    .absolute()
                    .left(size::COUNT_LEFT)
                    .bottom(size::COUNT_BOTTOM)
                    .px(size::COUNT_PAD_X)
                    .py(size::COUNT_PAD_Y)
                    .rounded(radius::SMALL)
                    .bg(colors.sheet)
                    .text_size(text::LABEL)
                    .font_weight(text::DETAIL_WEIGHT)
                    .text_color(colors.text_dim)
                    .child(line),
            )
        })
    }
}

/// A note's line, its Undo when it has one, and a button to close it.
fn note_parts(note: &Note, colors: &Colors, cx: &Context<MainWindow>) -> Vec<gpui::AnyElement> {
    let mut parts = vec![label(colors).child(note.label.clone()).into_any_element()];
    if note.undo.is_some() {
        parts.push(
            text_button("island-undo", UNDO_LABEL, &Undo, colors.accept, colors).into_any_element(),
        );
    }
    parts.push(
        square("island-dismiss", colors)
            .child(icon(Icon::Close, size::BUTTON_ICON, colors.text_soft))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.dismiss_note(cx)))
            .into_any_element(),
    );
    parts
}

/// The island itself, centred over the bottom of the sheet, holding
/// `parts`. It takes the pointer, so a click on it never reaches a row
/// under it.
fn floating(parts: Vec<gpui::AnyElement>, colors: &Colors) -> impl IntoElement {
    div()
        .absolute()
        .left_0()
        .right_0()
        .bottom(size::ISLAND_BOTTOM)
        .flex()
        .justify_center()
        .px(size::ISLAND_MARGIN)
        .child(
            div()
                .id("island")
                .debug_selector(|| "island".to_owned())
                .occlude()
                .flex()
                .items_center()
                .min_w_0()
                .gap(size::ISLAND_GAP)
                .h(size::ISLAND_HEIGHT)
                .pl(size::ISLAND_PAD_LEFT)
                .pr(size::ISLAND_PAD_RIGHT)
                .rounded(radius::CARD)
                .bg(colors.lifted)
                .border(size::LINE)
                .border_color(colors.line)
                .shadow(vec![BoxShadow {
                    color: Hsla::from(colors.shadow),
                    offset: point(px(0.0), size::ISLAND_SHADOW_Y),
                    blur_radius: size::ISLAND_SHADOW_BLUR,
                    spread_radius: px(0.0),
                }])
                .children(parts),
        )
}

/// The island's words, which give way first on a narrow sheet.
fn label(colors: &Colors) -> Div {
    div()
        .flex()
        .min_w_0()
        .mr(size::ISLAND_TEXT_GAP)
        .truncate()
        .text_color(colors.text_soft)
}

/// The hairline between the actions and Clear selection.
fn divider(colors: &Colors) -> impl IntoElement {
    div()
        .flex_none()
        .w(size::LINE)
        .h(size::ISLAND_DIVIDER_HEIGHT)
        .mx(size::ISLAND_GAP)
        .bg(colors.line)
}

/// A square button on the island, with selector `name`.
fn square(name: &'static str, colors: &Colors) -> Stateful<Div> {
    div()
        .id(name)
        .debug_selector(|| name.to_owned())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(size::ICON_BUTTON)
        .rounded(radius::ROW)
        .cursor_pointer()
        .hover(|button| button.bg(colors.hover))
        .active(|button| button.bg(colors.press))
}

/// An icon button that sends `action`.
fn icon_button(
    name: &'static str,
    glyph: Icon,
    action: &dyn Action,
    colors: &Colors,
) -> impl IntoElement {
    let action = action.boxed_clone();
    square(name, colors)
        .child(icon(glyph, size::BUTTON_ICON, colors.text_soft))
        .on_click(move |_: &ClickEvent, window, cx| {
            window.dispatch_action(action.boxed_clone(), cx);
        })
}

/// A worded button in `tint` that sends `action`.
fn text_button(
    name: &'static str,
    words: &'static str,
    action: &dyn Action,
    tint: Rgba,
    colors: &Colors,
) -> impl IntoElement {
    let action = action.boxed_clone();
    square(name, colors)
        .w_auto()
        .px(size::TEXT_BUTTON_PAD)
        .font_weight(text::STRONG_WEIGHT)
        .text_color(tint)
        .child(SharedString::new_static(words))
        .on_click(move |_: &ClickEvent, window, cx| {
            window.dispatch_action(action.boxed_clone(), cx);
        })
}
