use std::path::PathBuf;

use gpui::{
    Action, ClickEvent, Context, Div, InteractiveElement as _, IntoElement, MouseButton,
    MouseDownEvent, ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _,
    Styled as _, div, prelude::FluentBuilder as _, transparent_black,
};
use nova_files_api::Gathering;

use super::bar::capsule;
use super::drop::inside_edge;
use super::place_icon;
use super::row::icon;
use nova_files_shell::{Look, MenuAt};

use crate::assets::Icon;
use crate::commands::EmptyTrash;
use crate::crumbs::{self, Crumb, Shown};
use crate::path_bar::{MenuItem, MenuOwner};
use crate::places::{self, TRASH_LABEL};
use crate::theme::{Colors, radius, size, text};
use crate::window::{Aim, MainWindow};

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
        if let Some(place) = self.gathered_here() {
            let chip = self.gathered_chip(place, colors, cx);
            return bar
                .child(gathered_crumb(place, colors, cx))
                .child(div().flex_1())
                .children(self.scope_chip(colors, cx))
                .child(self.drop_target(chip, Aim::gathered(place), cx));
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
                        Aim::trash()
                    } else {
                        Aim::folder(crumb.path.clone(), crumb.label.clone())
                    };
                    if crumb.here && self.properties_paths().is_some() {
                        row = row.child(crumb_button(index, crumb, false, colors, cx));
                        continue;
                    }
                    let button = crumb_button(index, crumb, true, colors, cx);
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
            .children(
                self.parts
                    .menu(MenuAt::PathBarEnd, &self.scene(), cx)
                    .into_iter()
                    .flatten()
                    .map(|item| part_chip(item, colors)),
            )
    }

    fn crumbs(&self) -> Vec<Crumb> {
        if let Some(paths) = self.properties_paths() {
            return self.properties_crumbs(paths);
        }
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

    fn properties_crumbs(&self, paths: &[PathBuf]) -> Vec<Crumb> {
        let shown = match paths {
            [one] => one.as_path(),
            _ => match self.properties_folder() {
                Some(folder) => folder,
                None => return Vec::new(),
            },
        };
        let mut crumbs = crumbs::crumbs(shown, &self.home, &self.places, place_icon);
        if let (Some(last), Some(icon)) = (crumbs.last_mut(), self.properties_icon()) {
            last.icon = icon;
        }
        crumbs
    }

    fn menu_open(&self, owner: MenuOwner) -> bool {
        self.path_menu().is_some_and(|menu| menu.owner == owner)
    }

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

fn crumb_button(
    index: usize,
    crumb: &Crumb,
    goes: bool,
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
        .when(goes, |button| {
            button.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.close_path_menu();
                this.open(&target);
                cx.notify();
            }))
        })
        .when(crumb.shows_icon, |button| {
            button.child(icon(crumb.icon, size::CRUMB_ICON, tint))
        })
        .child(crumb.label.clone())
}

fn gathered_crumb(place: Gathering, colors: &Colors, cx: &Context<MainWindow>) -> Stateful<Div> {
    let (label, glyph) = places::gathering_look(place);
    crumb_box("crumb-0".into(), colors)
        .bg(colors.hover)
        .text_color(colors.text)
        .font_weight(text::STRONG_WEIGHT)
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.open_gathered(place, cx);
        }))
        .child(icon(glyph, size::CRUMB_ICON, colors.here))
        .child(label)
}

impl MainWindow {
    fn gathered_chip(
        &self,
        place: Gathering,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id("gathered-folders")
            .debug_selector(|| "gathered-folders".to_owned())
            .flex()
            .flex_none()
            .items_center()
            .h(size::CHIP_HEIGHT)
            .px(inside_edge(size::CHIP_PAD))
            .rounded(radius::ROW)
            .text_color(colors.text_soft)
            .cursor_pointer()
            .hover(|chip| chip.bg(colors.hover).text_color(colors.text))
            .active(|chip| chip.bg(colors.press))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.open_gathered_menu(place, event.position, cx);
                }),
            )
            .child(self.gathered_count(place))
    }
}

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
        .px(inside_edge(size::CRUMB_PAD))
        .border(size::DROP_EDGE)
        .border_color(transparent_black())
        .rounded(radius::ROW)
        .whitespace_nowrap()
        .text_color(colors.text_dim)
        .cursor_pointer()
        .hover(|button| button.bg(colors.hover).text_color(colors.text))
}

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

fn part_chip(item: nova_files_shell::MenuItem, colors: &Colors) -> impl IntoElement {
    let selector = item.selector();
    let action = item.run.action();
    chip(selector, colors)
        .when(item.look == Look::Danger, |chip| chip.text_color(colors.danger))
        .child(item.label)
        .on_click(move |_: &ClickEvent, window, cx| {
            window.dispatch_action(action.boxed_clone(), cx);
        })
}

fn chip(selector: &'static str, colors: &Colors) -> Stateful<Div> {
    div()
        .id(selector)
        .debug_selector(move || selector.to_owned())
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
}

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
