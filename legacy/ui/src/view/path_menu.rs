use gpui::{
    AnyElement, ClickEvent, Context, Corner, Div, InteractiveElement as _, IntoElement,
    MouseDownEvent, ParentElement as _, Stateful, StatefulInteractiveElement as _, Styled as _,
    anchored, deferred, div, point, prelude::FluentBuilder as _, px, uniform_list,
};

use super::row::icon;
use crate::path_bar::MenuItem;
use crate::theme::{Colors, radius, size, text};
use crate::window::MainWindow;

const MENU_PRIORITY: usize = 1;

impl MainWindow {
    pub(super) fn path_menu_card(&self, cx: &Context<Self>) -> AnyElement {
        let colors = Colors::of(cx);
        let items = self.path_menu().map_or(&[][..], |menu| &menu.items);
        let widest = items
            .iter()
            .enumerate()
            .max_by_key(|(_, item)| item.label.chars().count())
            .map(|(index, _)| index);
        let list = uniform_list(
            "path-menu-items",
            items.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                let colors = Colors::of(cx);
                let Some(menu) = this.path_menu() else {
                    return Vec::new();
                };
                range
                    .filter_map(|index| menu.items.get(index).map(|item| (index, item)))
                    .map(|(index, item)| menu_item(index, item, &colors, cx))
                    .collect()
            }),
        )
        .with_sizing_behavior(gpui::ListSizingBehavior::Infer)
        .with_width_from_item(widest)
        .min_w(size::MENU_MIN_WIDTH)
        .max_w(size::MENU_MAX_WIDTH)
        .max_h(size::MENU_MAX_HEIGHT);
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
            .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                this.dismiss_path_menu(event.position);
                cx.notify();
            }))
            .child(list);
        div()
            .absolute()
            .bottom_0()
            .left_0()
            .child(
                deferred(
                    anchored()
                        .anchor(Corner::TopLeft)
                        .offset(point(px(0.0), size::MENU_OFFSET))
                        .snap_to_window()
                        .child(card),
                )
                .with_priority(MENU_PRIORITY),
            )
            .into_any_element()
    }
}

fn menu_item(
    index: usize,
    item: &MenuItem,
    colors: &Colors,
    cx: &Context<MainWindow>,
) -> Stateful<Div> {
    let tint = if item.on {
        colors.here
    } else {
        colors.text_dim
    };
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
        .cursor_pointer()
        .hover(|row| row.bg(colors.press).text_color(colors.text))
        .when(item.on, |row| {
            row.text_color(colors.text).font_weight(text::STRONG_WEIGHT)
        })
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.choose_path_menu_item(index);
            cx.notify();
        }))
        .child(icon(item.icon, size::CRUMB_ICON, tint))
        .child(div().min_w_0().truncate().child(item.label.clone()))
}
