use gpui::{
    ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div,
};

use super::row::icon;
use crate::assets::Icon;
use crate::theme::{Colors, radius, size, text};
use crate::window::MainWindow;

const FILTER_SELECTOR: &str = "filter";
const CLEAR_SELECTOR: &str = "filter-clear";

impl MainWindow {
    pub(super) fn filter_chip(&self, cx: &Context<Self>) -> Option<impl IntoElement> {
        if !self.filtering() {
            return None;
        }
        let filter = self.filter();
        let colors = Colors::of(cx);
        let shown = self
            .listing
            .as_ref()
            .map_or(0, |listing| listing.entries.len());
        let clear = div()
            .id(CLEAR_SELECTOR)
            .debug_selector(|| CLEAR_SELECTOR.to_owned())
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(size::FILTER_CLEAR)
            .rounded(radius::SMALL)
            .cursor_pointer()
            .hover(|button| button.bg(colors.press))
            .child(icon(Icon::Close, size::FILTER_CLEAR_ICON, colors.text_dim))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.clear_filter(cx)));
        Some(
            div()
                .debug_selector(|| FILTER_SELECTOR.to_owned())
                .absolute()
                .left(size::COUNT_LEFT)
                .bottom(size::COUNT_BOTTOM)
                .flex()
                .items_center()
                .gap(size::COUNT_PAD_X)
                .pl(size::COUNT_PAD_X)
                .pr(size::COUNT_PAD_Y)
                .py(size::COUNT_PAD_Y)
                .rounded(radius::SMALL)
                .bg(colors.raised)
                .text_size(text::LABEL)
                .font_weight(text::DETAIL_WEIGHT)
                .text_color(colors.text_dim)
                .child(
                    div()
                        .text_color(colors.text)
                        .font_weight(text::STRONG_WEIGHT)
                        .child(filter.text.clone()),
                )
                .child(shown.to_string())
                .child(clear),
        )
    }
}
