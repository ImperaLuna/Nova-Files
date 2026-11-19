use gpui::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, linear_color_stop, linear_gradient,
};

use crate::theme::{Colors, radius, start};
use crate::window::MainWindow;
pub(super) use nova_files_shell::paths::{name_of, name_os, where_is};
pub(super) use nova_files_shell::widgets::{FAN_ENTRIES, fan, space_bar as bar, titles};

impl MainWindow {
    pub(super) fn start_page(&self, cx: &Context<Self>) -> impl IntoElement {
        let colors = Colors::of(cx);
        div()
            .id("start")
            .debug_selector(|| "start".to_owned())
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .rounded_tl(radius::SHEET)
            .bg(linear_gradient(
                start::GLOW_ANGLE,
                linear_color_stop(colors.glow, 0.0),
                linear_color_stop(colors.sheet, start::GLOW_END),
            ))
            .child(
                div()
                    .mx_auto()
                    .w_full()
                    .max_w(start::MAX_WIDTH)
                    .pt(start::PAD_TOP)
                    .px(start::PAD_SIDE)
                    .flex()
                    .flex_col()
                    .gap(start::GROUP_GAP)
                    .children(self.parts.start_cards()),
            )
    }
}
