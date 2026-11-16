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
//! The empty part of the bar after them always shows; a click there turns
//! the bar into a text field to type a path in (see `path_bar`). A
//! feature's button (search scope, Empty Trash) goes after it, at the
//! bar's right end.
//!
//! Debug selectors: `crumb-<n>` for each crumb, counted from the root
//! whether shown or folded, `arrow-<n>` for the arrow after it, and
//! `crumb-more` for the "…" crumb, and `path-fill` for the empty part.

use std::path::PathBuf;

use gpui::{
    Action, ClickEvent, Context, Div, InteractiveElement as _, IntoElement, MouseButton,
    MouseDownEvent, ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _,
    Styled as _, div, prelude::FluentBuilder as _, transparent_black,
};

use super::bar::capsule;
use super::drop::inside_edge;
use super::place_icon;
use super::row::icon;
use crate::assets::Icon;
use crate::commands::EmptyTrash;
use crate::crumbs::{self, Crumb, Shown};
use crate::dropping::DropAt;
use crate::path_bar::{MenuItem, MenuOwner};
use crate::places::TRASH_LABEL;
use crate::theme::{Colors, radius, size, text};
use crate::window::MainWindow;

const FOLDED_LABEL: &str = "…";
const EMPTY_TRASH_LABEL: &str = "Empty Trash";

impl MainWindow {
    pub(super) fn path_bar(&self, colors: &Colors, cx: &Context<Self>) -> impl IntoElement {
        let bar = capsule(colors)
            .debug_selector(|| "path".to_owned())
            .flex_1()
            .min_w(size::PATH_MIN_WIDTH)
            .px(size::CAPSULE_PAD);
        if let Some(typing) = &self.path_bar.field {
            return bar.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .px(size::PATH_FIELD_PAD)
                    .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                        this.stop_typing_path(Some(window), cx);
                    }))
                    .child(typing.field.clone()),
            );
        }
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
                    let at = if self.trash_open() {
                        DropAt::Trash
                    } else {
                        DropAt::Folder(crumb.path.clone())
                    };
                    let button = crumb_button(index, crumb, colors, cx);
                    row = row.child(self.drop_target(button, at, cx));
                    if !crumb.here || (self.has_folders() && !self.trash_open()) {
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
        let fill = div()
            .id("path-fill")
            .debug_selector(|| "path-fill".to_owned())
            .flex_1()
            .self_stretch()
            .min_w(size::PATH_FILL_MIN)
            .cursor_text()
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                this.start_typing_path(window, cx);
            }));
        let empties = self.trash_open() && self.trash_count().is_some_and(|count| count > 0);
        bar.child(row)
            .child(fill)
            .children(self.scope_chip(colors, cx))
            .when(empties, |bar| bar.child(empty_trash_chip(colors)))
    }

    /// The crumbs of the folder on screen; none before one arrives. The
    /// trash is one crumb, Trash, whichever folder holds it.
    fn crumbs(&self) -> Vec<Crumb> {
        let Some(folder) = self.folder() else {
            return Vec::new();
        };
        if self.trash_open() {
            return vec![Crumb {
                path: folder.to_path_buf(),
                label: SharedString::new_static(TRASH_LABEL),
                icon: Icon::Trash,
                shows_icon: true,
                here: true,
            }];
        }
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
        // A clear drop edge (see `drop`), inside the padding, so every
        // crumb lines up whether it takes drops or not.
        .px(inside_edge(size::CRUMB_PAD))
        .border(size::DROP_EDGE)
        .border_color(transparent_black())
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

/// Empty Trash, at the path bar's right end in the trash; it asks
/// first (see `window::trash`).
fn empty_trash_chip(colors: &Colors) -> impl IntoElement {
    div()
        .id("empty-trash")
        .debug_selector(|| "empty-trash".to_owned())
        .flex()
        .flex_none()
        .items_center()
        .h(size::CHIP_HEIGHT)
        .px(size::CHIP_PAD)
        .rounded(radius::ROW)
        .text_color(colors.text_soft)
        .cursor_pointer()
        .hover(|chip| chip.bg(colors.hover).text_color(colors.text))
        .active(|chip| chip.bg(colors.press))
        .child(EMPTY_TRASH_LABEL)
        .on_click(|_: &ClickEvent, window, cx| {
            window.dispatch_action(EmptyTrash.boxed_clone(), cx);
        })
}
