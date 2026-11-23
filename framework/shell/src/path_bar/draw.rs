use std::path::PathBuf;

use gpui::{
    Anchor, AnyElement, App, ClickEvent, Div, Entity, InteractiveElement as _, IntoElement,
    MouseButton, MouseDownEvent, ParentElement as _, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, anchored, deferred, div, point,
    prelude::FluentBuilder as _, px, transparent_black, uniform_list,
};

use super::crumbs::{self, Crumb, Shown};
use super::{MenuItem, MenuOwner, PathBar, Spot};
use crate::assets::Icon;
use crate::menu::{Look, MenuItem as CommandItem};
use crate::text::TextField;
use crate::theme::{Colors, radius, size, text};
use crate::widgets::{capsule, icon};

const FOLDED_LABEL: &str = "…";
const MENU_PRIORITY: usize = 1;

pub struct BarInput {
    pub on_screen: Option<PathBuf>,
    pub asks: u64,
    pub crumbs: Vec<Crumb>,
    pub arrow_here: bool,
    pub chips: Vec<AnyElement>,
}

#[derive(Clone)]
struct Here {
    on_screen: Option<PathBuf>,
    asks: u64,
}

impl Here {
    fn spot(&self) -> Spot<'_> {
        Spot {
            on_screen: self.on_screen.as_deref(),
            asks: self.asks,
        }
    }
}

pub type Drops<'a> = &'a dyn Fn(Stateful<Div>, &Crumb) -> Stateful<Div>;

pub fn draw(bar: &Entity<PathBar>, input: BarInput, drops: Drops<'_>, cx: &App) -> Stateful<Div> {
    let colors = Colors::of(cx);
    let shell = capsule(&colors)
        .debug_selector(|| "path".to_owned())
        .flex_1()
        .min_w(size::PATH_MIN_WIDTH)
        .px(size::CAPSULE_PAD);
    if let Some(field) = bar.read(cx).field() {
        return shell.id("path-shell").child(typing(bar, field.clone()));
    }
    let BarInput {
        on_screen,
        asks,
        crumbs,
        arrow_here,
        chips,
    } = input;
    let here = Here {
        on_screen: on_screen.clone(),
        asks,
    };
    let mut row = div()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .flex()
        .flex_shrink_1()
        .min_w_0()
        .overflow_hidden()
        .justify_end()
        .items_center();
    for shown in crumbs::shown(crumbs.len()) {
        match shown {
            Shown::Crumb(index) => {
                let crumb = &crumbs[index];
                let Some(opens) = crumb.opens.clone() else {
                    row = row.child(crumb_box(index, crumb, &colors));
                    continue;
                };
                let going = bar.clone();
                let button =
                    crumb_box(index, crumb, &colors).on_click(move |_: &ClickEvent, _, cx| {
                        going.update(cx, |bar, cx| {
                            bar.close_menu(cx);
                            bar.go(opens.clone(), cx);
                        });
                    });
                row = row.child(drops(button, crumb));
                if let Some(folder) = crumb.folder()
                    && (!crumb.here || arrow_here)
                {
                    let arrow = arrow(bar, (index, folder.to_path_buf()), &here, &colors, cx);
                    row = row.child(arrow);
                }
            }
            Shown::Folded(hidden) => {
                let folded = crumbs[hidden].to_vec();
                row = row
                    .child(folded_crumb(bar, folded, &here, &colors, cx))
                    .child(chevron(&colors));
            }
        }
    }
    let chips = div()
        .flex()
        .items_center()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .children(chips);
    types_on_click(shell, bar, on_screen)
        .child(row)
        .child(fill())
        .child(chips)
}

fn typing(bar: &Entity<PathBar>, field: Entity<TextField>) -> Div {
    let typing = bar.clone();
    div()
        .flex_1()
        .min_w_0()
        .px(size::PATH_FIELD_PAD)
        .on_mouse_down_out(move |_: &MouseDownEvent, _, cx| {
            typing.update(cx, |bar, cx| bar.stop_typing(true, cx));
        })
        .child(field)
}

fn fill() -> Div {
    div()
        .debug_selector(|| "path-fill".to_owned())
        .flex_1()
        .self_stretch()
        .min_w(size::PATH_FILL_MIN)
}

fn types_on_click(shell: Div, bar: &Entity<PathBar>, on_screen: Option<PathBuf>) -> Stateful<Div> {
    let shell = shell.id("path-shell");
    match on_screen {
        Some(folder) => {
            let typing = bar.clone();
            shell
                .cursor_text()
                .on_click(move |_: &ClickEvent, window, cx| {
                    typing.update(cx, |bar, cx| bar.start_typing(&folder, window, cx));
                })
        }
        None => shell,
    }
}

fn crumb_box(index: usize, crumb: &Crumb, colors: &Colors) -> Stateful<Div> {
    let tint = if crumb.here {
        colors.here
    } else {
        colors.text_dim
    };
    plain_crumb(format!("crumb-{index}").into(), colors)
        .when(crumb.here, |button| {
            button
                .bg(colors.hover)
                .text_color(colors.text)
                .font_weight(text::STRONG_WEIGHT)
        })
        .when(crumb.shows_icon, |button| {
            button.child(icon(crumb.icon, size::CRUMB_ICON, tint))
        })
        .child(crumb.label.clone())
}

fn plain_crumb(selector: SharedString, colors: &Colors) -> Stateful<Div> {
    let name = selector.to_string();
    div()
        .id(selector)
        .debug_selector(move || name)
        .flex()
        .flex_none()
        .items_center()
        .gap(size::CRUMB_GAP)
        .h(size::ICON_BUTTON)
        .px(size::CRUMB_PAD - size::DROP_EDGE)
        .border(size::DROP_EDGE)
        .border_color(transparent_black())
        .rounded(radius::ROW)
        .whitespace_nowrap()
        .text_color(colors.text_dim)
        .cursor_default()
        .hover(|button| button.bg(colors.hover).text_color(colors.text))
}

fn folded_crumb(
    bar: &Entity<PathBar>,
    folded: Vec<Crumb>,
    here: &Here,
    colors: &Colors,
    cx: &App,
) -> Div {
    let open = menu_open(bar, MenuOwner::Folded, here, cx);
    let (pressed, screen) = (bar.clone(), here.clone());
    let button = plain_crumb("crumb-more".into(), colors)
        .when(open, |button| button.bg(colors.press))
        .on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, _, cx| {
            pressed.update(cx, |bar, cx| {
                bar.press_folded(event.position, &folded, screen.spot(), cx);
            });
        })
        .child(FOLDED_LABEL);
    div()
        .relative()
        .flex_none()
        .child(button)
        .when(open, |slot| slot.child(menu_card(bar, here, cx)))
}

fn arrow(
    bar: &Entity<PathBar>,
    (index, folder): (usize, PathBuf),
    here: &Here,
    colors: &Colors,
    cx: &App,
) -> Div {
    let open = menu_open(bar, MenuOwner::Arrow(index), here, cx);
    let (pressed, screen) = (bar.clone(), here.clone());
    let button = chevron(colors)
        .id(("arrow", index))
        .debug_selector(move || format!("arrow-{index}"))
        .rounded(radius::SMALL)
        .cursor_pointer()
        .hover(|button| button.bg(colors.press))
        .when(open, |button| button.bg(colors.press))
        .on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, _, cx| {
            pressed.update(cx, |bar, cx| {
                bar.press_arrow((index, &folder), event.position, screen.spot(), cx);
            });
        });
    div()
        .relative()
        .flex_none()
        .child(button)
        .when(open, |slot| slot.child(menu_card(bar, here, cx)))
}

fn menu_open(bar: &Entity<PathBar>, owner: MenuOwner, here: &Here, cx: &App) -> bool {
    bar.read(cx)
        .menu(here.spot())
        .is_some_and(|menu| menu.owner == owner)
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

fn menu_card(bar: &Entity<PathBar>, here: &Here, cx: &App) -> AnyElement {
    let colors = Colors::of(cx);
    let items = bar
        .read(cx)
        .menu(here.spot())
        .map(|menu| menu.items.clone())
        .unwrap_or_default();
    let widest = items
        .iter()
        .enumerate()
        .max_by_key(|(_, item)| item.label.chars().count())
        .map(|(index, _)| index);
    let (listed, chosen) = (bar.clone(), here.clone());
    let list = uniform_list(
        "path-menu-items",
        items.len(),
        move |range: std::ops::Range<usize>, _, cx| {
            let colors = Colors::of(cx);
            range
                .filter_map(|index| items.get(index).map(|item| (index, item)))
                .map(|(index, item)| menu_item(&listed, index, item, &chosen, &colors))
                .collect::<Vec<_>>()
        },
    )
    .with_sizing_behavior(gpui::ListSizingBehavior::Infer)
    .with_width_from_item(widest)
    .min_w(size::MENU_MIN_WIDTH)
    .max_w(size::MENU_MAX_WIDTH)
    .max_h(size::MENU_MAX_HEIGHT);
    let (dismissed, screen) = (bar.clone(), here.clone());
    let card = div()
        .id("path-menu")
        .debug_selector(|| "path-menu".to_owned())
        .p(size::MENU_PAD)
        .rounded(radius::CARD)
        .border(size::LINE)
        .border_color(colors.line)
        .bg(colors.lifted)
        .text_color(colors.text_soft)
        .font_weight(text::BODY_WEIGHT)
        .on_mouse_down_out(move |event: &MouseDownEvent, _, cx| {
            dismissed.update(cx, |bar, cx| {
                bar.dismiss_menu(event.position, screen.spot(), cx);
            });
        })
        .child(list);
    div()
        .absolute()
        .bottom_0()
        .left_0()
        .child(
            deferred(
                anchored()
                    .anchor(Anchor::TopLeft)
                    .offset(point(px(0.0), size::MENU_OFFSET))
                    .snap_to_window()
                    .child(card),
            )
            .with_priority(MENU_PRIORITY),
        )
        .into_any_element()
}

fn menu_item(
    bar: &Entity<PathBar>,
    index: usize,
    item: &MenuItem,
    here: &Here,
    colors: &Colors,
) -> Stateful<Div> {
    let tint = if item.on {
        colors.here
    } else {
        colors.text_dim
    };
    let (chosen, screen) = (bar.clone(), here.clone());
    div()
        .id(("path-menu-item", index))
        .debug_selector(move || format!("path-menu-{index}"))
        .flex()
        .items_center()
        .gap(size::MENU_ICON_GAP)
        .w_full()
        .h(size::MENU_ROW_HEIGHT)
        .px(size::MENU_ITEM_PAD)
        .rounded(radius::ROW)
        .whitespace_nowrap()
        .hover(|row| row.bg(colors.press).text_color(colors.text))
        .when(item.on, |row| {
            row.text_color(colors.text).font_weight(text::STRONG_WEIGHT)
        })
        .on_click(move |_: &ClickEvent, _, cx| {
            chosen.update(cx, |bar, cx| bar.choose(index, screen.spot(), cx));
        })
        .child(icon(item.icon, size::CRUMB_ICON, tint))
        .child(div().min_w_0().truncate().child(item.label.clone()))
}

pub fn menu_chip(item: CommandItem, colors: &Colors) -> impl IntoElement {
    let selector = item.selector();
    let action = item.run.action();
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
        .when(item.look == Look::Danger, |chip| {
            chip.text_color(colors.danger)
        })
        .child(item.label)
        .on_click(move |_: &ClickEvent, window, cx| {
            window.dispatch_action(action.boxed_clone(), cx);
        })
}
