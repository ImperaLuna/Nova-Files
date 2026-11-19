use std::f32::consts::FRAC_PI_2;

use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent,
    ParentElement as _, Styled as _, Transformation, div, radians, svg,
};
use nova_files_properties_api::{PathKind, Properties};

use nova_files_shell::assets::Icon;
use nova_files_shell::theme::{Colors, properties, radius, start, text};
use nova_files_shell::widgets::{card_group as group, icon};

use super::lone_glyph;
use crate::PropertiesPart;

const LABEL: &str = "Opens with";
const NO_DEFAULT: &str = "None";

pub(super) fn card(
    part: &PropertiesPart,
    one: &Properties,
    cx: &Context<PropertiesPart>,
) -> Option<AnyElement> {
    if one.kind != PathKind::File {
        return None;
    }
    if part.apps_to_open_with().is_empty() {
        return None;
    }
    let name = part
        .default_app()
        .map_or_else(|| NO_DEFAULT.into(), |app| app.name.clone());
    let colors = Colors::of(cx);
    let card = div()
        .id("opens-with")
        .debug_selector(|| "opens-with".to_owned())
        .flex()
        .items_center()
        .gap(properties::CHOICE_GAP)
        .min_w_0()
        .p(start::CARD_PAD)
        .rounded(radius::CARD)
        .bg(colors.raised)
        .cursor_pointer()
        .hover(|card| card.bg(colors.lifted))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                this.open_apps_menu(event.position, cx);
            }),
        )
        .child(icon(lone_glyph(one), start::CARD_ICON, colors.text_soft))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(text::CARD_NAME)
                .font_weight(text::STRONG_WEIGHT)
                .text_color(colors.text)
                .child(name),
        )
        .child(
            svg()
                .path(Icon::Chevron.path())
                .flex_none()
                .size(properties::CHOICE_ARROW)
                .text_color(gpui::Hsla::from(colors.text_faint))
                .with_transformation(Transformation::rotate(radians(FRAC_PI_2))),
        );
    Some(group(LABEL, card.into_any_element(), &colors).into_any_element())
}
