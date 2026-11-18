//! The top bar: a row of capsules on the window. Back, Forward and Up;
//! the path bar (see `path`), which takes what the others leave, with the
//! search's scope chip at its end; the search box (see `search`); and the
//! list and tiles switch, which shows Details or Icons as Ctrl+1 and
//! Ctrl+2 do (see `window::views`), lit on the one showing.

use gpui::{
    Action, ClickEvent, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _,
};

use super::row::icon;
use nova_files_api::ViewMode;

use crate::assets::Icon;
use crate::commands::{GoBack, GoForward, GoUp, ShowDetails, ShowIcons};
use crate::theme::{Colors, radius, size};
use crate::window::MainWindow;

impl MainWindow {
    pub(super) fn top_bar(&self, colors: &Colors, cx: &Context<Self>) -> impl IntoElement {
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
            .child(self.path_bar(colors, cx))
            .child(
                self.search_box(
                    capsule(colors)
                        .debug_selector(|| "search".to_owned())
                        .flex_shrink()
                        .w(size::SEARCH_WIDTH)
                        .px(size::SEARCH_PAD)
                        .gap(size::SEARCH_GAP),
                    colors,
                    cx,
                ),
            )
            .child(
                capsule(colors)
                    .debug_selector(|| "views".to_owned())
                    .px(size::CAPSULE_PAD)
                    .gap(size::CAPSULE_BUTTON_GAP)
                    .children(self.view_switch(colors)),
            )
    }
}

/// One capsule of the bar, raised from the window.
pub(super) fn capsule(colors: &Colors) -> Div {
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
pub(super) fn icon_button(
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

impl MainWindow {
    /// The list and tiles switch, lit on the view showing. Search results
    /// and the trash show Details only, so there a click does nothing.
    fn view_switch(&self, colors: &Colors) -> Vec<gpui::Stateful<Div>> {
        let showing = self.folder_view().mode;
        [
            (
                "view-list",
                Icon::List,
                ViewMode::Details,
                &ShowDetails as &dyn Action,
            ),
            ("view-tiles", Icon::Tiles, ViewMode::Icons, &ShowIcons),
        ]
        .into_iter()
        .map(|(name, glyph, mode, action)| {
            let on = showing == mode;
            switch_button(name, glyph, on, colors).when(!on, |button| {
                let action = action.boxed_clone();
                button
                    .cursor_pointer()
                    .hover(|button| button.bg(colors.hover))
                    .on_click(move |_: &ClickEvent, window, cx| {
                        window.dispatch_action(action.boxed_clone(), cx);
                    })
            })
        })
        .collect()
    }
}

/// One side of the list and tiles switch; `on` marks the view showing.
fn switch_button(
    name: &'static str,
    glyph: Icon,
    on: bool,
    colors: &Colors,
) -> gpui::Stateful<Div> {
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
