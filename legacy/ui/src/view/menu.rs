use gpui::{
    Action, AnyElement, BoxShadow, ClickEvent, Context, Div, Hsla, InteractiveElement as _,
    IntoElement, KeyBinding, ParentElement as _, Rgba, Stateful, StatefulInteractiveElement as _,
    Styled as _, Window, anchored, deferred, div, point, prelude::FluentBuilder as _, px,
};

use super::row::icon;
use crate::menu::{
    self, Look, Menu, MenuAction, MenuChoose, MenuClose, MenuDown, MenuItem, MenuUp, key_label,
};
use crate::theme::{Colors, radius, size, text};
use crate::window::MainWindow;

const MENU_PRIORITY: usize = 1;

impl MainWindow {
    pub(super) fn menu_overlay(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> Option<impl IntoElement> {
        let menu = self.menu.as_ref()?;
        let Menu {
            at,
            items,
            lines,
            focus,
            ..
        } = menu;
        let colors = Colors::of(cx);
        let mut rows: Vec<AnyElement> = Vec::new();
        for (index, item) in items.iter().enumerate() {
            if lines.contains(&index) {
                rows.push(menu_line(index, &colors).into_any_element());
            }
            let marked = menu.marks(index);
            rows.push(
                menu_item(index, item, marked, &colors, cx)
                    .children(
                        item_keys(item, |action| {
                            window.highest_precedence_binding_for_action(action)
                        })
                        .or_else(|| item.detail.clone())
                        .map(|keys| key_cell(keys, &colors)),
                    )
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

fn menu_card(colors: &Colors) -> Stateful<Div> {
    div()
        .id("menu")
        .debug_selector(|| "menu".to_owned())
        .occlude()
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

fn item_keys(
    item: &MenuItem,
    bound: impl FnOnce(&dyn Action) -> Option<KeyBinding>,
) -> Option<gpui::SharedString> {
    let MenuAction::Run(spec) = item.action else {
        return None;
    };
    bound(&*spec.action()).and_then(|binding| key_label(&binding))
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ItemStyle {
    tint: Rgba,
    name: Rgba,
    strong: bool,
}

impl ItemStyle {
    fn of(look: Look, marked: bool, colors: &Colors) -> Self {
        let (tint, name) = match look {
            Look::Plain if marked => (colors.text_dim, colors.text),
            Look::Plain => (colors.text_dim, colors.text_soft),
            Look::On => (colors.here, colors.text),
            Look::Danger => (colors.danger, colors.danger),
        };
        Self {
            tint,
            name,
            strong: look == Look::On,
        }
    }
}

fn menu_line(index: usize, colors: &Colors) -> Div {
    div()
        .debug_selector(move || format!("menu-line-{index}"))
        .flex_none()
        .h(size::LINE)
        .mx(size::MENU_LINE_X)
        .my(size::MENU_LINE_Y)
        .bg(colors.line)
}

fn menu_item(
    index: usize,
    item: &MenuItem,
    marked: bool,
    colors: &Colors,
    cx: &Context<MainWindow>,
) -> Stateful<Div> {
    let style = ItemStyle::of(item.look, marked, colors);
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
        .text_color(style.name)
        .when(style.strong, |row| row.font_weight(text::STRONG_WEIGHT))
        .when(marked, |row| row.bg(colors.press))
        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            if *hovered {
                this.mark_menu_item(index, cx);
            }
        }))
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.choose(index, window, cx);
        }))
        .child(icon(item.icon, size::MENU_ICON, style.tint))
        .child(div().min_w_0().truncate().child(item.label.clone()))
}

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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::assets::Icon;
    use crate::commands::COPY;

    #[test]
    fn an_item_shows_the_keys_of_its_action() {
        let copy = MenuItem::run(Icon::Copy, &COPY);
        let bound = |_: &dyn Action| Some(COPY.binding(COPY.keys[0]));
        assert_eq!(
            item_keys(&copy, bound).map(String::from).as_deref(),
            Some("Ctrl C")
        );
        assert_eq!(item_keys(&copy, |_| None), None);
        let unpin = MenuItem::new(Icon::Trash, "Remove", MenuAction::Unpin(PathBuf::new()));
        assert_eq!(item_keys(&unpin, bound), None);
    }

    #[test]
    fn a_look_sets_the_colors_and_weight() {
        let colors = Colors::default();
        let style = |look, marked| ItemStyle::of(look, marked, &colors);
        assert_eq!(style(Look::Plain, false).name, colors.text_soft);
        assert_eq!(style(Look::Plain, true).name, colors.text);
        assert!(!style(Look::Plain, true).strong);
        assert_eq!(style(Look::On, false).tint, colors.here);
        assert!(style(Look::On, false).strong);
        let danger = style(Look::Danger, true);
        assert_eq!((danger.name, danger.tint), (colors.danger, colors.danger));
        assert!(!danger.strong);
    }
}
