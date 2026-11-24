use gpui::{
    BoxShadow, Div, ElementId, Entity, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _,
    Transformation, div, point, prelude::FluentBuilder as _, px, radians, svg,
};

use crate::assets::Icon;
use crate::text::TextField;
use crate::theme::{Colors, radius, size, text};

const POINT_UP: f32 = 270.0_f32.to_radians();
const POINT_DOWN: f32 = 90.0_f32.to_radians();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Width {
    List,
    Fit,
}

pub fn floating(
    parts: Vec<gpui::AnyElement>,
    above: Option<gpui::AnyElement>,
    colors: &Colors,
) -> impl IntoElement {
    floating_at(parts, above, Width::List, colors)
}

pub fn floating_at(
    parts: Vec<gpui::AnyElement>,
    above: Option<gpui::AnyElement>,
    width: Width,
    colors: &Colors,
) -> impl IntoElement {
    let row = div()
        .flex()
        .items_center()
        .min_w_0()
        .gap(size::ISLAND_GAP)
        .children(parts);
    div()
        .absolute()
        .left_0()
        .right_0()
        .bottom(size::ISLAND_BOTTOM)
        .flex()
        .justify_center()
        .px(size::ISLAND_MARGIN)
        .child(
            div()
                .id("island")
                .debug_selector(|| "island".to_owned())
                .occlude()
                .flex()
                .min_w_0()
                .map(|island| match above {
                    Some(above) => island
                        .flex_col()
                        .map(|island| match width {
                            Width::List => island.w(size::ISLAND_LIST_WIDTH),
                            Width::Fit => island.min_w(size::ISLAND_LIST_WIDTH),
                        })
                        .p(size::ISLAND_LIST_PAD)
                        .child(above)
                        .child(row.h(size::ISLAND_LIST_LINE).pl(size::ISLAND_LIST_INSET)),
                    None => island
                        .items_center()
                        .h(size::ISLAND_HEIGHT)
                        .pl(size::ISLAND_PAD_LEFT)
                        .pr(size::ISLAND_PAD_RIGHT)
                        .child(row),
                })
                .rounded(radius::CARD)
                .bg(colors.lifted)
                .border(size::LINE)
                .border_color(colors.line)
                .shadow(vec![BoxShadow {
                    color: Hsla::from(colors.shadow),
                    offset: point(px(0.0), size::ISLAND_SHADOW_Y),
                    blur_radius: size::ISLAND_SHADOW_BLUR,
                    spread_radius: px(0.0),
                    inset: false,
                }]),
        )
}

pub fn label(colors: &Colors) -> Div {
    div()
        .flex()
        .min_w_0()
        .mr(size::ISLAND_TEXT_GAP)
        .truncate()
        .text_color(colors.text_soft)
}

pub fn divider(colors: &Colors) -> impl IntoElement {
    div()
        .flex_none()
        .w(size::LINE)
        .h(size::ISLAND_DIVIDER_HEIGHT)
        .mx(size::ISLAND_GAP)
        .bg(colors.line)
}

pub fn square(name: impl Into<SharedString>, colors: &Colors) -> Stateful<Div> {
    let name = name.into();
    div()
        .id(ElementId::Name(name.clone()))
        .debug_selector(move || name.to_string())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(size::ICON_BUTTON)
        .rounded(radius::ROW)
        .cursor_pointer()
        .hover(|button| button.bg(colors.hover))
        .active(|button| button.bg(colors.press))
}

pub fn worded(
    selector: &'static str,
    words: impl Into<SharedString>,
    colors: &Colors,
) -> Stateful<Div> {
    square(selector, colors)
        .w_auto()
        .px(size::TEXT_BUTTON_PAD)
        .font_weight(text::STRONG_WEIGHT)
        .child(words.into())
}

pub fn password_box(field: Entity<TextField>, colors: &Colors) -> Div {
    div()
        .debug_selector(|| "island-password".to_owned())
        .flex()
        .flex_none()
        .items_center()
        .w(size::PASSWORD_WIDTH)
        .h(size::RENAME_HEIGHT)
        .px(size::RENAME_PAD)
        .rounded(radius::SMALL)
        .bg(colors.window)
        .border(size::RING)
        .border_color(colors.accept)
        .text_color(colors.text)
        .child(field)
}

pub fn chevron(open: bool, colors: &Colors) -> impl IntoElement {
    let angle = if open { POINT_DOWN } else { POINT_UP };
    svg()
        .path(Icon::Chevron.path())
        .flex_none()
        .size(size::BUTTON_ICON)
        .text_color(Hsla::from(colors.text_soft))
        .with_transformation(Transformation::rotate(radians(angle)))
}
