//! Draws the window's menu (see `menu`) over everything else, at the
//! point it opened, kept inside the window. Its items carry the debug
//! selectors `menu` and `menu-<n>`, counted from 0.

use gpui::{
    AnyElement, ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, anchored, deferred, div,
};

use super::row::icon;
use crate::menu::Menu;
use crate::theme::{Colors, radius, size};
use crate::window::MainWindow;

/// The menu draws over the sheet and the sidebar.
const MENU_PRIORITY: usize = 1;

impl MainWindow {
    /// The menu, while one is open.
    pub(super) fn menu_overlay(&self, cx: &Context<Self>) -> Option<impl IntoElement> {
        let Menu { at, items } = self.menu.as_ref()?;
        let colors = Colors::of(cx);
        let items: Vec<AnyElement> = items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                div()
                    .id(("menu", index))
                    .debug_selector(move || format!("menu-{index}"))
                    .flex()
                    .items_center()
                    .gap(size::PLACE_PAD)
                    .h(size::MENU_ROW_HEIGHT)
                    .px(size::PLACE_PAD)
                    .rounded(radius::ROW)
                    .cursor_pointer()
                    .text_color(colors.text_soft)
                    .hover(|row| row.bg(colors.hover).text_color(colors.text))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.choose(index, cx);
                    }))
                    .child(icon(item.icon, size::MENU_ICON, colors.text_dim))
                    .child(item.label.clone())
                    .into_any_element()
            })
            .collect();
        Some(
            deferred(
                anchored().position(*at).snap_to_window().child(
                    div()
                        .id("menu")
                        .debug_selector(|| "menu".to_owned())
                        .flex()
                        .flex_col()
                        .gap(size::PLACE_GAP)
                        .min_w(size::MENU_MIN_WIDTH)
                        .p(size::MENU_PAD)
                        .rounded(radius::CARD)
                        .border(size::LINE)
                        .border_color(colors.line)
                        .bg(colors.lifted)
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_menu(cx)))
                        .children(items),
                ),
            )
            .with_priority(MENU_PRIORITY),
        )
    }
}
