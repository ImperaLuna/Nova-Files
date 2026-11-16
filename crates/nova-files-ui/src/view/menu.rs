//! Draws the window's menu (see `menu`) over everything else, its top
//! left corner at the point it opened, moved back inside the window when
//! it would cross an edge. Each item is its icon and name, with the key
//! that does the same at its right; a line runs between groups. The
//! marked item (by key or pointer) is lit; an item that is on is strong
//! with an orange icon, and one that removes something is in the error
//! color. While open, the menu holds the keys (see `menu`). Its items
//! carry the debug selectors `menu` and `menu-<n>`, counted from 0, and
//! its lines `menu-line-<n>`, by the item they sit above.

use gpui::{
    AnyElement, BoxShadow, ClickEvent, Context, Div, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, Stateful, StatefulInteractiveElement as _, Styled as _, Window, anchored,
    deferred, div, point, prelude::FluentBuilder as _, px,
};

use super::row::icon;
use crate::menu::{
    self, Look, Menu, MenuAction, MenuChoose, MenuClose, MenuDown, MenuItem, MenuUp, key_label,
};
use crate::theme::{Colors, radius, size, text};
use crate::window::MainWindow;

/// The menu draws over the sheet and the sidebar.
const MENU_PRIORITY: usize = 1;

impl MainWindow {
    /// The menu, while one is open.
    pub(super) fn menu_overlay(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> Option<impl IntoElement> {
        let Menu {
            at,
            items,
            lines,
            active,
            focus,
            ..
        } = self.menu.as_ref()?;
        let colors = Colors::of(cx);
        let mut rows: Vec<AnyElement> = Vec::new();
        for (index, item) in items.iter().enumerate() {
            if lines.contains(&index) {
                rows.push(menu_line(index, &colors).into_any_element());
            }
            let marked = *active == Some(index);
            rows.push(
                menu_item(index, item, marked, &colors, cx)
                    .children(item_keys(item, window).map(|keys| key_cell(keys, &colors)))
                    .into_any_element(),
            );
        }
        Some(
            deferred(
                anchored()
                    .position(*at)
                    .snap_to_window_with_margin(size::MENU_EDGE)
                    .child(
                        menu_card(&colors)
                            .track_focus(focus)
                            .key_context(menu::KEY_CONTEXT)
                            .on_action(cx.listener(|this, _: &MenuUp, _, cx| {
                                this.step_menu(true, cx);
                            }))
                            .on_action(cx.listener(|this, _: &MenuDown, _, cx| {
                                this.step_menu(false, cx);
                            }))
                            .on_action(cx.listener(|this, _: &MenuChoose, window, cx| {
                                this.choose_marked(window, cx);
                            }))
                            .on_action(cx.listener(|this, _: &MenuClose, _, cx| {
                                this.close_menu(cx);
                            }))
                            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_menu(cx)))
                            .children(rows),
                    ),
            )
            .with_priority(MENU_PRIORITY),
        )
    }
}

/// The card the items sit on.
fn menu_card(colors: &Colors) -> Stateful<Div> {
    div()
        .id("menu")
        .debug_selector(|| "menu".to_owned())
        .flex()
        .flex_col()
        .gap(size::MENU_ITEM_GAP)
        .min_w(size::MENU_MIN_WIDTH)
        .max_w(size::MENU_MAX_WIDTH)
        .p(size::MENU_PAD)
        .rounded(radius::CARD)
        .border(size::LINE)
        .border_color(colors.line)
        .bg(colors.lifted)
        .shadow(vec![BoxShadow {
            color: Hsla::from(colors.shadow),
            offset: point(px(0.0), size::MENU_SHADOW_Y),
            blur_radius: size::MENU_SHADOW_BLUR,
            spread_radius: px(0.0),
        }])
}

/// The keys that do what `item` does, when it sends one of the window's
/// actions and a key is bound to it.
fn item_keys(item: &MenuItem, window: &Window) -> Option<gpui::SharedString> {
    let MenuAction::Run(spec) = item.action else {
        return None;
    };
    window
        .highest_precedence_binding_for_action(&*spec.action())
        .and_then(|binding| key_label(&binding))
}

/// The line above item `index`, between two groups.
fn menu_line(index: usize, colors: &Colors) -> Div {
    div()
        .debug_selector(move || format!("menu-line-{index}"))
        .flex_none()
        .h(size::LINE)
        .mx(size::MENU_LINE_X)
        .my(size::MENU_LINE_Y)
        .bg(colors.line)
}

/// Item `index`: its icon and its name; its keys go at the right (see
/// [`key_cell`]).
fn menu_item(
    index: usize,
    item: &MenuItem,
    marked: bool,
    colors: &Colors,
    cx: &Context<MainWindow>,
) -> Stateful<Div> {
    let (tint, name) = match item.look {
        Look::Plain => (colors.text_dim, colors.text_soft),
        Look::On => (colors.here, colors.text),
        Look::Danger => (colors.danger, colors.danger),
    };
    div()
        .id(("menu", index))
        .debug_selector(move || format!("menu-{index}"))
        .flex()
        .flex_none()
        .items_center()
        .gap(size::MENU_ICON_GAP)
        .h(size::MENU_ROW_HEIGHT)
        .px(size::MENU_ITEM_PAD)
        .rounded(radius::ROW)
        .whitespace_nowrap()
        .cursor_pointer()
        .text_color(name)
        .when(item.look == Look::On, |row| {
            row.font_weight(text::STRONG_WEIGHT)
        })
        .when(marked, |row| {
            row.bg(colors.press)
                .when(item.look == Look::Plain, |row| row.text_color(colors.text))
        })
        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            if *hovered {
                this.mark_menu_item(index, cx);
            }
        }))
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.choose(index, window, cx);
        }))
        .child(icon(item.icon, size::MENU_ICON, tint))
        .child(div().min_w_0().truncate().child(item.label.clone()))
}

/// An item's keys, at its right.
fn key_cell(keys: gpui::SharedString, colors: &Colors) -> Div {
    div()
        .flex_none()
        .ml_auto()
        .pl(size::MENU_KEY_GAP)
        .text_size(text::LABEL)
        .font_weight(text::DETAIL_WEIGHT)
        .text_color(colors.text_dim)
        .child(keys)
}
