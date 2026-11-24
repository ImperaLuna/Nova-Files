use gpui::{
    AnyElement, BoxShadow, ClickEvent, Context, Div, Entity, Hsla, InteractiveElement as _,
    IntoElement, MouseButton, MouseDownEvent, ParentElement as _, Pixels, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled, deferred, div, point, prelude::FluentBuilder as _, px,
};

use super::row::icon;
use crate::assets::Icon;
use crate::shortcuts::{KEY_CONTEXT, Picking, ShortcutDown, ShortcutUp, Shortcuts};
use crate::text::TextField;
use crate::theme::{Colors, radius, size, text};
use crate::window::MainWindow;

const SHORTCUTS_PRIORITY: usize = 2;
const SHORTCUTS_WIDTH: Pixels = px(520.0);
const SHORTCUTS_TOP: Pixels = px(56.0);
const SHORTCUTS_LIST_HEIGHT: Pixels = px(420.0);
const SHORTCUTS_FIELD_HEIGHT: Pixels = px(44.0);
const SHORTCUTS_HEAD_TOP: Pixels = px(10.0);
const KEY_CAP_HEIGHT: Pixels = px(20.0);
const KEY_CAP_PAD: Pixels = px(6.0);
const KEY_CAP_GAP: Pixels = px(3.0);
const KEY_CHOICE_GAP: Pixels = px(6.0);
const KEY_CHOICE: &str = "or";

impl MainWindow {
    pub(super) fn shortcuts_overlay(&self, cx: &Context<Self>) -> Option<impl IntoElement> {
        let Shortcuts {
            picking,
            field,
            scroll,
            ..
        } = self.shortcuts.as_ref()?;
        let colors = Colors::of(cx);
        Some(
            deferred(
                div()
                    .absolute()
                    .top(SHORTCUTS_TOP)
                    .left_0()
                    .right_0()
                    .px(size::MENU_EDGE)
                    .flex()
                    .justify_center()
                    .child(
                        card(&colors)
                            .key_context(KEY_CONTEXT)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                                    this.focus_shortcut_field(window, cx);
                                }),
                            )
                            .on_action(cx.listener(|this, _: &ShortcutUp, _, cx| {
                                this.step_shortcut(true, cx);
                            }))
                            .on_action(cx.listener(|this, _: &ShortcutDown, _, cx| {
                                this.step_shortcut(false, cx);
                            }))
                            .on_mouse_down_out(cx.listener(
                                |this, _: &MouseDownEvent, window, cx| {
                                    this.close_shortcuts(window, cx);
                                    cx.stop_propagation();
                                },
                            ))
                            .child(field_bar(field, &colors))
                            .child(
                                div()
                                    .id("shortcuts-list")
                                    .debug_selector(|| "shortcuts-list".to_owned())
                                    .flex()
                                    .flex_col()
                                    .gap(size::MENU_ITEM_GAP)
                                    .max_h(SHORTCUTS_LIST_HEIGHT)
                                    .overflow_y_scroll()
                                    .track_scroll(scroll)
                                    .p(size::MENU_PAD)
                                    .children(rows(picking, &colors, cx)),
                            ),
                    ),
            )
            .with_priority(SHORTCUTS_PRIORITY),
        )
    }
}

fn rows(picking: &Picking, colors: &Colors, cx: &Context<MainWindow>) -> Vec<AnyElement> {
    let mut group = None;
    picking
        .shown
        .iter()
        .enumerate()
        .map(|(at, &index)| {
            let shortcut = &picking.list[index];
            let heading = (picking.grouped && group != Some(shortcut.group))
                .then(|| group_head(shortcut.group.label(), colors));
            group = Some(shortcut.group);
            div()
                .flex()
                .flex_col()
                .children(heading)
                .child(shortcut_row(picking, at, colors, cx))
                .into_any_element()
        })
        .collect()
}

fn field_bar(field: &Entity<TextField>, colors: &Colors) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(size::SEARCH_GAP)
        .h(SHORTCUTS_FIELD_HEIGHT)
        .px(size::SEARCH_PAD)
        .border_b(size::LINE)
        .border_color(colors.line)
        .child(icon(Icon::Search, size::MENU_ICON, colors.text_dim))
        .child(div().flex_1().min_w_0().child(field.clone()))
}

fn card(colors: &Colors) -> Stateful<Div> {
    div()
        .id("shortcuts")
        .debug_selector(|| "shortcuts".to_owned())
        .occlude()
        .flex()
        .flex_col()
        .w(SHORTCUTS_WIDTH)
        .max_w_full()
        .rounded(radius::CARD)
        .border(size::LINE)
        .border_color(colors.line)
        .bg(colors.lifted)
        .overflow_hidden()
        .shadow(vec![BoxShadow {
            color: Hsla::from(colors.shadow),
            offset: point(px(0.0), size::MENU_SHADOW_Y),
            blur_radius: size::MENU_SHADOW_BLUR,
            spread_radius: px(0.0),
            inset: false,
        }])
}

fn group_head(label: &'static str, colors: &Colors) -> Div {
    div()
        .flex_none()
        .pt(SHORTCUTS_HEAD_TOP)
        .pb(size::MENU_LINE_Y)
        .px(size::MENU_ITEM_PAD)
        .text_size(text::LABEL)
        .font_weight(text::LABEL_WEIGHT)
        .text_color(colors.text_dim)
        .child(label)
}

fn shortcut_row(
    picking: &Picking,
    at: usize,
    colors: &Colors,
    cx: &Context<MainWindow>,
) -> Stateful<Div> {
    let shortcut = &picking.list[picking.shown[at]];
    let marked = picking.marked == Some(at);
    let named_group = !picking.grouped;
    let name = match (shortcut.runs, marked) {
        (false, _) => colors.text_faint,
        (true, true) => colors.text,
        (true, false) => colors.text_soft,
    };
    let runs = shortcut.runs;
    div()
        .id(("shortcut", at))
        .debug_selector(move || format!("shortcut-{at}"))
        .flex()
        .flex_none()
        .items_center()
        .gap(size::MENU_ICON_GAP)
        .h(size::MENU_ROW_HEIGHT)
        .px(size::MENU_ITEM_PAD)
        .rounded(radius::ROW)
        .whitespace_nowrap()
        .text_color(name)
        .when(runs, Styled::cursor_pointer)
        .when(marked, |row| row.bg(colors.press))
        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            if *hovered {
                this.mark_shortcut(at, cx);
            }
        }))
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.run_shortcut(at, window, cx);
        }))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(shortcut.spec.label),
        )
        .when(named_group, |row| {
            row.child(
                div()
                    .flex_none()
                    .text_size(text::LABEL)
                    .font_weight(text::DETAIL_WEIGHT)
                    .text_color(colors.text_faint)
                    .child(shortcut.group.label()),
            )
        })
        .child(key_choices(&shortcut.keys, runs, colors))
}

fn key_choices(keys: &[SharedString], runs: bool, colors: &Colors) -> Div {
    let tint = if runs {
        colors.text_soft
    } else {
        colors.text_faint
    };
    let mut row = div().flex().flex_none().items_center().gap(KEY_CHOICE_GAP);
    for (index, keys) in keys.iter().enumerate() {
        if index > 0 {
            row = row.child(
                div()
                    .text_size(text::LABEL)
                    .font_weight(text::DETAIL_WEIGHT)
                    .text_color(colors.text_faint)
                    .child(KEY_CHOICE),
            );
        }
        row = row.child(
            div().flex().gap(KEY_CAP_GAP).children(
                keys.split(' ')
                    .map(|key| key_cap(SharedString::from(key.to_owned()), tint, colors)),
            ),
        );
    }
    row
}

fn key_cap(key: SharedString, tint: gpui::Rgba, colors: &Colors) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .h(KEY_CAP_HEIGHT)
        .min_w(KEY_CAP_HEIGHT)
        .px(KEY_CAP_PAD)
        .rounded(radius::SMALL)
        .bg(colors.press)
        .text_size(text::LABEL)
        .font_weight(text::DETAIL_WEIGHT)
        .text_color(tint)
        .child(key)
}
