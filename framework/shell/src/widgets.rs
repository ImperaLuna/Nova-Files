use gpui::{
    Div, Hsla, IntoElement, ParentElement as _, Pixels, Rgba, Styled as _, div,
    prelude::FluentBuilder as _, svg,
};

use crate::assets::Icon;
use crate::theme::{Colors, radius, size};

pub fn icon(glyph: Icon, side: Pixels, tint: Rgba) -> impl IntoElement {
    svg()
        .path(glyph.path())
        .flex_none()
        .size(side)
        .text_color(Hsla::from(tint))
}

pub fn tick_box(ticked: bool, colors: &Colors) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(size::TICK_BOX)
        .rounded(radius::TICK)
        .border(size::TICK_LINE)
        .map(|tick| {
            if ticked {
                tick.bg(colors.text_soft)
                    .border_color(colors.text_soft)
                    .child(icon(Icon::Check, size::TICK_ICON, colors.lifted))
            } else {
                tick.border_color(colors.text_faint)
            }
        })
}
