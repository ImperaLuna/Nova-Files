//! The path bar, in the top bar's path slot: a crumb for each level from
//! Home or Computer down to the folder on screen (see `crumbs`), with an
//! arrow between each two. A click on a crumb goes to its folder. The
//! folder on screen is pressed in, strong, with its icon in orange; the
//! root shows its icon too. A press on an arrow opens a menu of the
//! folders in the crumb before it; the folder on screen has an arrow only
//! when it holds folders. A long path folds its middle into a "…" crumb
//! whose menu lists the folders it hides (see `path_menu`).
//!
//! The crumbs sit at the bar's right end of their own box, so a narrow
//! window cuts the root side off and keeps the folder on screen in view.
//! The empty part of the bar after them is kept free; a feature's button
//! (search scope, Empty Trash) goes after it, at the bar's right end.
//!
//! Debug selectors: `crumb-<n>` for each crumb, counted from the root
//! whether shown or folded, `arrow-<n>` for the arrow after it, and
//! `crumb-more` for the "…" crumb.

use std::path::PathBuf;

use gpui::{
    ClickEvent, Context, Div, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent,
    ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, div,
    prelude::FluentBuilder as _,
};

use super::bar::capsule;
use super::place_icon;
use super::row::icon;
use crate::assets::Icon;
use crate::crumbs::{self, Crumb, Shown};
use crate::path_bar::{MenuItem, MenuOwner};
use crate::theme::{Colors, radius, size, text};
use crate::window::MainWindow;

const FOLDED_LABEL: &str = "…";

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
        for shown in crumbs::shown(crumbs.len()) {
            match shown {
                Shown::Crumb(index) => {
                    let crumb = &crumbs[index];
                    row = row.child(crumb_button(index, crumb, colors, cx));
                    if !crumb.here || self.has_folders() {
                        row = row.child(self.arrow(index, crumb.path.clone(), colors, cx));
                    }
                }
                Shown::Folded(hidden) => {
                    let items = crumbs[hidden].iter().cloned().map(MenuItem::from);
                    row = row
                        .child(self.folded_crumb(items.collect(), colors, cx))
                        .child(chevron(colors));
                }
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

    /// Whether `owner`'s menu is the one open.
    fn menu_open(&self, owner: MenuOwner) -> bool {
        self.path_menu().is_some_and(|menu| menu.owner == owner)
    }

    /// The "…" crumb, which opens `items`, the folders it hides, as a
    /// menu.
    fn folded_crumb(&self, items: Vec<MenuItem>, colors: &Colors, cx: &Context<Self>) -> Div {
        let open = self.menu_open(MenuOwner::Folded);
        let button = crumb_box("crumb-more".into(), colors)
            .when(open, |button| button.bg(colors.press))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.press_path_menu(MenuOwner::Folded, event.position, items.clone());
                    cx.notify();
                }),
            )
            .child(FOLDED_LABEL);
        div()
            .relative()
            .flex_none()
            .child(button)
            .when(open, |slot| slot.child(self.path_menu_card(cx)))
    }

    /// The arrow after the `index`th crumb, whose menu lists the folders
    /// in `folder`.
    fn arrow(&self, index: usize, folder: PathBuf, colors: &Colors, cx: &Context<Self>) -> Div {
        let open = self.menu_open(MenuOwner::Arrow(index));
        let button = chevron(colors)
            .id(("arrow", index))
            .debug_selector(move || format!("arrow-{index}"))
            .rounded(radius::SMALL)
            .cursor_pointer()
            .hover(|button| button.bg(colors.press))
            .when(open, |button| button.bg(colors.press))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.press_arrow(index, &folder, event.position, cx);
                    cx.notify();
                }),
            );
        div()
            .relative()
            .flex_none()
            .child(button)
            .when(open, |slot| slot.child(self.path_menu_card(cx)))
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
            this.close_path_menu();
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

/// An arrow between two crumbs.
fn chevron(colors: &Colors) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(size::SEPARATOR_WIDTH)
        .h(size::SEPARATOR_HEIGHT)
        .child(icon(Icon::Chevron, size::SEPARATOR_ICON, colors.text_faint))
}
