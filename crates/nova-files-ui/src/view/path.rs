//! The path bar, in the top bar's path slot: a crumb for each level from
//! Home or Computer down to the folder on screen (see `crumbs`), with an
//! arrow between each two. A click on a crumb goes to its folder. The
//! folder on screen is pressed in, strong, with its icon in orange; the
//! root shows its icon too.
//!
//! The crumbs sit at the bar's right end of their own box, so a narrow
//! window cuts the root side off and keeps the folder on screen in view.
//! The empty part of the bar after them is kept free; a feature's button
//! (search scope, Empty Trash) goes after it, at the bar's right end.
//!
//! Debug selectors: `crumb-<n>` for each crumb, counted from the root, and
//! `arrow-<n>` for the arrow after it.

use gpui::{
    ClickEvent, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, div,
    prelude::FluentBuilder as _,
};

use super::bar::capsule;
use super::row::icon;
use super::sidebar::place_icon;
use crate::assets::Icon;
use crate::crumbs::{self, Crumb};
use crate::theme::{Colors, radius, size, text};
use crate::window::MainWindow;

impl MainWindow {
    pub(super) fn path_bar(&self, colors: &Colors, cx: &Context<Self>) -> impl IntoElement {
        let crumbs = self.crumbs();
        let mut row = div()
            .flex()
            .flex_shrink()
            .min_w_0()
            .overflow_hidden()
            .justify_end()
            .items_center();
        for (index, crumb) in crumbs.iter().enumerate() {
            row = row.child(crumb_button(index, crumb, colors, cx));
            if !crumb.here {
                row = row.child(separator(index, colors));
            }
        }
        capsule(colors)
            .debug_selector(|| "path".to_owned())
            .flex_1()
            .min_w(size::PATH_MIN_WIDTH)
            .px(size::CAPSULE_PAD)
            .child(row)
            .child(div().flex_1().self_stretch().min_w(size::PATH_FILL_MIN))
    }

    /// The crumbs of the folder on screen; none before one arrives.
    fn crumbs(&self) -> Vec<Crumb> {
        let Some(folder) = self.folder() else {
            return Vec::new();
        };
        crumbs::crumbs(folder, &self.home, &self.places, place_icon)
    }
}

/// The `index`th crumb, which goes to its folder. The folder on screen is
/// pressed in with its icon orange.
fn crumb_button(
    index: usize,
    crumb: &Crumb,
    colors: &Colors,
    cx: &Context<MainWindow>,
) -> Stateful<Div> {
    let target = crumb.path.clone();
    let tint = if crumb.here {
        colors.here
    } else {
        colors.text_dim
    };
    crumb_box(format!("crumb-{index}").into(), colors)
        .when(crumb.here, |button| {
            button
                .bg(colors.hover)
                .text_color(colors.text)
                .font_weight(text::STRONG_WEIGHT)
        })
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.open(&target);
            cx.notify();
        }))
        .when(crumb.shows_icon, |button| {
            button.child(icon(crumb.icon, size::CRUMB_ICON, tint))
        })
        .child(crumb.label.clone())
}

/// A crumb's box, with `selector` as its id and debug selector.
fn crumb_box(selector: SharedString, colors: &Colors) -> Stateful<Div> {
    let name = selector.to_string();
    div()
        .id(selector)
        .debug_selector(move || name)
        .flex()
        .flex_none()
        .items_center()
        .gap(size::CRUMB_GAP)
        .h(size::ICON_BUTTON)
        .px(size::CRUMB_PAD)
        .rounded(radius::ROW)
        .whitespace_nowrap()
        .text_color(colors.text_dim)
        .cursor_pointer()
        .hover(|button| button.bg(colors.hover).text_color(colors.text))
}

/// The arrow after the `index`th crumb.
fn separator(index: usize, colors: &Colors) -> Div {
    div()
        .debug_selector(move || format!("arrow-{index}"))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(size::SEPARATOR_WIDTH)
        .h(size::SEPARATOR_HEIGHT)
        .child(icon(Icon::Chevron, size::SEPARATOR_ICON, colors.text_faint))
}
