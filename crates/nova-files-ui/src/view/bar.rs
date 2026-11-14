//! The top bar: a row of capsules on the window. Back, Forward and Up;
//! the path's slot, which takes what the others leave; the search slot;
//! and the list and tiles switch. The path bar, search and the tiles view
//! are features of their own that fill their slot; until then the path's
//! slot shows the folder's path, the search slot its label, and the slots
//! do nothing.

use gpui::{
    Action, ClickEvent, Div, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _,
};

use super::row::icon;
use crate::assets::Icon;
use crate::commands::{GoBack, GoForward, GoUp};
use crate::theme::{Colors, radius, size};
use crate::window::MainWindow;

const SEARCH_LABEL: &str = "Search";

impl MainWindow {
    pub(super) fn top_bar(&self, colors: &Colors) -> impl IntoElement {
        let back = icon_button(
            "back",
            Icon::Back,
            self.back_target().is_some().then_some(&GoBack),
            colors,
        );
        let forward = icon_button(
            "forward",
            Icon::Forward,
            self.forward_target().is_some().then_some(&GoForward),
            colors,
        );
        let up = icon_button(
            "up",
            Icon::Up,
            self.parent().is_some().then_some(&GoUp),
            colors,
        );
        let path = self
            .folder()
            .map(|folder| SharedString::from(folder.to_string_lossy().into_owned()))
            .unwrap_or_default();
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(size::GAP)
            .pr(size::GAP)
            .pb(size::GAP)
            .child(
                capsule(colors)
                    .px(size::CAPSULE_PAD)
                    .gap(size::CAPSULE_BUTTON_GAP)
                    .child(back)
                    .child(forward)
                    .child(up),
            )
            .child(
                capsule(colors)
                    .debug_selector(|| "path".to_owned())
                    .flex_1()
                    .min_w(size::PATH_MIN_WIDTH)
                    .px(size::PATH_PAD)
                    .child(div().min_w_0().truncate().child(path)),
            )
            .child(
                capsule(colors)
                    .debug_selector(|| "search".to_owned())
                    .flex_shrink()
                    .w(size::SEARCH_WIDTH)
                    .px(size::SEARCH_PAD)
                    .gap(size::SEARCH_GAP)
                    .text_color(colors.text_dim)
                    .child(icon(Icon::Search, size::BUTTON_ICON, colors.text_dim))
                    .child(div().min_w_0().truncate().child(SEARCH_LABEL)),
            )
            .child(
                capsule(colors)
                    .debug_selector(|| "views".to_owned())
                    .px(size::CAPSULE_PAD)
                    .gap(size::CAPSULE_BUTTON_GAP)
                    .child(switch_button("view-list", Icon::List, true, colors))
                    .child(switch_button("view-tiles", Icon::Tiles, false, colors)),
            )
    }
}

/// One capsule of the bar, raised from the window.
fn capsule(colors: &Colors) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(size::CAPSULE_HEIGHT)
        .rounded(radius::CONTROL)
        .bg(colors.raised)
}

/// An icon button in a capsule, with selector `name`. A click sends
/// `action`; without one the button is dimmed and does nothing.
fn icon_button(
    name: &'static str,
    glyph: Icon,
    action: Option<&dyn Action>,
    colors: &Colors,
) -> impl IntoElement {
    let tone = if action.is_some() {
        colors.text_soft
    } else {
        colors.text_faint
    };
    square(name)
        .child(icon(glyph, size::BUTTON_ICON, tone))
        .when_some(action.map(Action::boxed_clone), |button, action| {
            button
                .cursor_pointer()
                .hover(|button| button.bg(colors.hover))
                .active(|button| button.bg(colors.press))
                .on_click(move |_: &ClickEvent, window, cx| {
                    window.dispatch_action(action.boxed_clone(), cx);
                })
        })
}

/// One side of the list and tiles switch; `on` marks the view showing.
fn switch_button(name: &'static str, glyph: Icon, on: bool, colors: &Colors) -> impl IntoElement {
    let tone = if on { colors.text } else { colors.text_faint };
    square(name)
        .when(on, |button| button.bg(colors.press))
        .child(icon(glyph, size::BUTTON_ICON, tone))
}

/// A square icon button's box, with selector `name`.
fn square(name: &'static str) -> gpui::Stateful<Div> {
    div()
        .id(name)
        .debug_selector(|| name.to_owned())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(size::ICON_BUTTON)
        .rounded(radius::ROW)
}
