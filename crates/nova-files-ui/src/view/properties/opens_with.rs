//! The Properties page's Opens with group, for a lone file: one card
//! with the file's icon, the name of the app that opens it, and an arrow.
//! A press on it opens the menu of the apps that can (see
//! `window::opens_with`). Without a default app the group isn't drawn.
//!
//! Debug selector: `opens-with` for the card.

use std::f32::consts::FRAC_PI_2;

use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent,
    ParentElement as _, Styled as _, Transformation, div, radians, svg,
};
use nova_files_api::{PathKind, Properties};

use super::lone_glyph;
use crate::assets::Icon;
use crate::theme::{Colors, properties, radius, start, text};
use crate::view::row::icon;
use crate::view::start::group;
use crate::window::MainWindow;

const LABEL: &str = "Opens with";

impl MainWindow {
    /// The Opens with group for `one`, when it is a file with a default
    /// app.
    pub(super) fn opens_with(&self, one: &Properties, cx: &Context<Self>) -> Option<AnyElement> {
        if one.kind != PathKind::File {
            return None;
        }
        let app = self.default_app()?;
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
                    .child(app.name.clone()),
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
}
