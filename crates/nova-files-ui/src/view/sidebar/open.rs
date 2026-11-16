//! The Open list, fixed at the top of the sidebar: one row per tab, drawn
//! like a place (see `window/tabs`). The tab on screen is pressed in,
//! strong, its icon orange. A click shows a tab; middle-click closes it,
//! and so does its x, which shows on the tab on screen and on the one
//! under the pointer, and only while there is more than one tab. The + on
//! the head opens a new tab on Start; it is always there, even with one
//! tab. Many tabs scroll inside the list, which takes at most a share of
//! the sidebar's height, so Places keep their room.
//!
//! A narrow window shows the tabs side by side at the start of the folded
//! row, the + after them and a hairline before Places. A picker has no
//! tabs.
//!
//! Debug selectors: `tab-<n>` for each row, counted from 0, `tab-close-<n>`
//! for its x, and `new-tab` for the +.

use gpui::{
    ClickEvent, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, div,
    prelude::FluentBuilder as _, relative,
};

use super::{Row, inside_edge, on_middle_click, row, section_button, section_head};
use crate::assets::Icon;
use crate::location::Location;
use crate::theme::{Colors, radius, size};
use crate::view::row::icon;
use crate::window::MainWindow;

const LABEL: &str = "Open";

impl MainWindow {
    /// The Open list; without its head when `narrow`. None in a picker.
    pub(super) fn open_section(&self, narrow: bool, cx: &Context<Self>) -> Option<Div> {
        if self.pick.is_some() {
            return None;
        }
        let colors = Colors::of(cx);
        let rows = self
            .tab_looks()
            .into_iter()
            .enumerate()
            .map(|(index, look)| self.tab_row(index, look, narrow, cx));
        let new_tab = section_button("new-tab", Icon::Plus, &colors).on_click(
            cx.listener(|this, _: &ClickEvent, _, cx| this.new_tab(&Location::Start, cx)),
        );
        let list = div().flex().gap(size::PLACE_GAP).children(rows);
        if narrow {
            return Some(
                list.items_center()
                    .flex_none()
                    .child(new_tab)
                    .pr(size::GAP)
                    .border_r(size::LINE)
                    .border_color(colors.line),
            );
        }
        Some(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .min_h_0()
                .max_h(relative(size::OPEN_MAX_SHARE))
                .pb(size::GAP)
                .border_b(size::LINE)
                .border_color(colors.line)
                .child(section_head(
                    LABEL,
                    Some(new_tab.into_any_element()),
                    &colors,
                ))
                .child(
                    list.id("open-scroll")
                        .flex_col()
                        .min_h_0()
                        .overflow_y_scroll(),
                ),
        )
    }

    /// Tab `index`'s row, with its name and icon, `look`; with its x
    /// while there is more than one tab.
    fn tab_row(
        &self,
        index: usize,
        (name, glyph): (SharedString, Icon),
        narrow: bool,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let colors = Colors::of(cx);
        let closable = self.tab_count() > 1;
        let on = index == self.current_tab();
        let group = format!("tab-{index}");
        row(
            Row {
                id: ("tab", index).into(),
                selector: group.clone(),
                glyph,
                name,
                number: None,
                here: on,
                pressed: on,
            },
            narrow,
            &colors,
        )
        .group(group.clone())
        .when(!narrow, |row| row.pr(inside_edge(size::TAB_CLOSE_RIGHT)))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.switch_tab(index, cx)))
        .map(|row| on_middle_click(row, true, cx, move |this, cx| this.close_tab(index, cx)))
        .when(closable, |row| {
            row.child(
                close_button(index, on, &group, &colors).on_click(cx.listener(
                    move |this, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        this.close_tab(index, cx);
                    },
                )),
            )
        })
    }
}

/// Tab `index`'s x: always shown on the tab on screen (`on`), else only
/// while the pointer is over its row, `group`.
fn close_button(index: usize, on: bool, group: &str, colors: &Colors) -> Stateful<Div> {
    let selector = format!("tab-close-{index}");
    div()
        .id(("tab-close", index))
        .debug_selector(move || selector)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(size::TAB_CLOSE)
        .rounded(radius::SMALL)
        .text_color(colors.text_dim)
        .when(!on, |button| {
            button
                .invisible()
                .group_hover(group.to_owned(), gpui::Styled::visible)
        })
        .hover(|button| button.bg(colors.press).text_color(colors.text))
        .child(icon(Icon::Close, size::TAB_CLOSE_ICON, colors.text_dim))
}
