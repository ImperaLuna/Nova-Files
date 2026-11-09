//! Draws the pick bar along a picker window's bottom: the name box, Cancel
//! and the Open or Save button, and under them, when a save would land on
//! a file that is there, Replace and Keep both. Every element a test
//! clicks carries a debug selector: `pick-name`, `pick-cancel`,
//! `pick-accept`, `pick-replace` and `pick-keep-both`.

use gpui::{
    ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _,
};

use super::Picker;
use crate::theme::{Colors, radius, size};
use crate::window::MainWindow;

const CANCEL_LABEL: &str = "Cancel";
const REPLACE_LABEL: &str = "Replace";
const KEEP_BOTH_LABEL: &str = "Keep both";

impl MainWindow {
    pub fn pick_bar(pick: &Picker, cx: &Context<Self>) -> impl IntoElement {
        let colors = Colors::of(cx);
        let name = div()
            .debug_selector(|| "pick-name".to_owned())
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .h(size::CONTROL_HEIGHT)
            .px(size::PAD)
            .rounded(radius::BUTTON)
            .border(size::DIVIDER)
            .border_color(colors.divider)
            .bg(colors.surface)
            .child(pick.name.clone());
        let controls = div()
            .flex()
            .items_center()
            .gap(size::GAP)
            .child(name)
            .children(Self::filter_button(pick, cx))
            .child(
                button("pick-cancel", CANCEL_LABEL.into(), false, colors)
                    .on_click(cx.listener(|this, _: &ClickEvent, window, _| this.cancel(window))),
            )
            .child(
                button(
                    "pick-accept",
                    super::accept_label(&pick.request).into(),
                    true,
                    colors,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.accept(window, cx))),
            );
        let question = pick.conflict.as_ref().map(|conflict| {
            let keep_both = format!(
                "{KEEP_BOTH_LABEL} ({})",
                conflict.keep_both.to_string_lossy()
            );
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(size::GAP)
                .child(
                    button("pick-replace", REPLACE_LABEL.into(), false, colors).on_click(
                        cx.listener(|this, _: &ClickEvent, window, _| this.replace(window)),
                    ),
                )
                .child(
                    button("pick-keep-both", keep_both.into(), true, colors).on_click(cx.listener(
                        |this, _: &ClickEvent, _, cx| {
                            this.keep_both();
                            cx.notify();
                        },
                    )),
                )
        });
        div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(size::QUESTION_GAP)
            .p(size::PAD)
            .bg(colors.surface_sunk)
            .border_t(size::DIVIDER)
            .border_color(colors.divider)
            .child(controls)
            .children(question)
    }
}

/// A bar button; `accept` marks the one Enter does.
pub fn button(
    id: &'static str,
    label: SharedString,
    accept: bool,
    colors: Colors,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .debug_selector(move || id.to_owned())
        .flex()
        .flex_none()
        .items_center()
        .h(size::CONTROL_HEIGHT)
        .px(size::PAD)
        .rounded(radius::BUTTON)
        .cursor_pointer()
        .map(|button| {
            if accept {
                button.bg(colors.accept).text_color(colors.on_accept)
            } else {
                button
                    .border(size::DIVIDER)
                    .border_color(colors.divider)
                    .hover(|button| button.bg(colors.hover))
            }
        })
        .child(div().min_w_0().truncate().child(label))
}
