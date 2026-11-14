//! Draws the island over the bottom of the sheet (see `island`): a
//! running job's line and orange bar, a question with Cancel and its
//! answer, a note with its Undo, or the selection with what can be done
//! to it (in the trash, Restore). While it
//! is down, the sheet's quiet count shows at the bottom left instead.
//!
//! Its buttons send the window's actions, as keys do (see `commands`);
//! only closing a note, which no key does, is handled here. Each part a
//! test clicks carries a debug selector: `island`, `island-copy`,
//! `island-cut`, `island-rename`, `island-trash`, `island-clear`, `island-undo`,
//! `island-dismiss`, `island-progress`, `island-cancel`, `island-restore`,
//! `island-no`, `island-yes`, and the count, `count`.

use gpui::{
    Action, BoxShadow, ClickEvent, Context, Div, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, Rgba, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _,
    div, point, prelude::FluentBuilder as _, px, relative,
};

use super::row::icon;
use crate::assets::Icon;
use crate::commands::{
    ClearSelection, CopySelected, CutSelected, RenameSelected, RestoreSelected, TrashSelected, Undo,
};
use crate::island::progress::Running;
use crate::island::{self, Confirm, Note};
use crate::theme::{Colors, radius, size, text};
use crate::window::MainWindow;

const UNDO_LABEL: &str = "Undo";
const CANCEL_LABEL: &str = "Cancel";
const RESTORE_LABEL: &str = "Restore";

impl MainWindow {
    /// The island, or the quiet count while the island is down.
    pub(super) fn island(&self, cx: &Context<Self>) -> impl IntoElement {
        let colors = Colors::of(cx);
        let parts = if self.pick.is_some() {
            None
        } else if let Some((_, job)) = self.job_on_island(cx) {
            Some(job_parts(job, &colors, cx))
        } else if let Some(confirm) = &self.island.confirm {
            Some(confirm_parts(confirm, &colors, cx))
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

    /// "3 selected · 12 MB", then Copy, Cut, Rename (for one), Move to
    /// Trash and Clear; in the trash, Restore and Clear.
    fn selection_parts(&self, colors: &Colors) -> Vec<gpui::AnyElement> {
        let count = self.selected_count();
        let (number, rest) = island::selection_line(count, self.selected_bytes());
        let mut parts = vec![
            label(colors)
                .child(
                    div()
                        .text_color(colors.text)
                        .font_weight(text::STRONG_WEIGHT)
                        .child(number),
                )
                .child(rest)
                .into_any_element(),
        ];
        if self.trash_open() {
            parts.push(
                text_button(
                    "island-restore",
                    RESTORE_LABEL,
                    &RestoreSelected,
                    colors.accept,
                    colors,
                )
                .into_any_element(),
            );
        } else if !self.in_trash() {
            file_buttons(count, &mut parts, colors);
        }
        parts.extend([
            divider(colors).into_any_element(),
            icon_button("island-clear", Icon::Close, &ClearSelection, colors).into_any_element(),
        ]);
        parts
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

/// Copy, Cut, Rename (for one) and Move to Trash.
fn file_buttons(count: usize, parts: &mut Vec<gpui::AnyElement>, colors: &Colors) {
    parts.extend([
        icon_button("island-copy", Icon::Copy, &CopySelected, colors).into_any_element(),
        icon_button("island-cut", Icon::Cut, &CutSelected, colors).into_any_element(),
    ]);
    if count == 1 {
        parts.push(
            icon_button("island-rename", Icon::Rename, &RenameSelected, colors).into_any_element(),
        );
    }
    parts.push(icon_button("island-trash", Icon::Trash, &TrashSelected, colors).into_any_element());
}

/// A running job's line and bar, and Cancel while it waits.
fn job_parts(job: &Running, colors: &Colors, cx: &Context<MainWindow>) -> Vec<gpui::AnyElement> {
    let now = cx.background_executor().now();
    let mut parts = vec![
        label(colors).child(job.line(now)).into_any_element(),
        div()
            .debug_selector(|| "island-progress".to_owned())
            .flex_none()
            .w(size::PROGRESS_WIDTH)
            .h(size::PROGRESS_HEIGHT)
            .mr(size::PROGRESS_RIGHT)
            .rounded_full()
            .bg(colors.press)
            .overflow_hidden()
            .child(
                div()
                    .h_full()
                    .w(relative(job.fraction()))
                    .rounded_full()
                    .bg(colors.accept),
            )
            .into_any_element(),
    ];
    if job.waiting {
        parts.push(
            square("island-cancel", colors)
                .w_auto()
                .px(size::TEXT_BUTTON_PAD)
                .font_weight(text::STRONG_WEIGHT)
                .child(SharedString::new_static(CANCEL_LABEL))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.cancel_job(cx)))
                .into_any_element(),
        );
    }
    parts
}

/// A question's line, Cancel, and its answer in red.
fn confirm_parts(
    confirm: &Confirm,
    colors: &Colors,
    cx: &Context<MainWindow>,
) -> Vec<gpui::AnyElement> {
    let answer = |name: &'static str, words: &'static str, yes: bool| {
        square(name, colors)
            .w_auto()
            .px(size::TEXT_BUTTON_PAD)
            .font_weight(text::STRONG_WEIGHT)
            .when(yes, |button| button.text_color(colors.danger))
            .child(SharedString::new_static(words))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.answer_confirm(yes, cx)))
            .into_any_element()
    };
    vec![
        label(colors)
            .child(confirm.label.clone())
            .into_any_element(),
        answer("island-no", CANCEL_LABEL, false),
        answer("island-yes", confirm.yes, true),
    ]
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
