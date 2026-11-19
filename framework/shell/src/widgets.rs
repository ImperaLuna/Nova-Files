use gpui::{
    AnyElement, Div, Hsla, IntoElement, ParentElement as _, Pixels, Radians, Rgba, Styled as _,
    Transformation, div, prelude::FluentBuilder as _, relative, svg,
};
use nova_files_api::{EntryKind, IconKind};

use crate::assets::Icon;
use crate::kind::{built_in, kind_glyph};
use crate::theme::{Colors, radius, size, start, text};

pub fn icon(glyph: Icon, side: Pixels, tint: Rgba) -> impl IntoElement {
    svg()
        .path(glyph.path())
        .flex_none()
        .size(side)
        .text_color(Hsla::from(tint))
}

pub fn icon_slot(side: Pixels) -> Div {
    div()
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(side)
}

pub fn kind_icon(kind: IconKind, side: Pixels, colors: &Colors) -> Div {
    let (glyph, tint) = built_in(kind, colors);
    icon_slot(side).child(icon(glyph, side, tint))
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

pub fn capsule(colors: &Colors) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(size::CAPSULE_HEIGHT)
        .rounded(radius::CONTROL)
        .bg(colors.raised)
}

pub fn card_group(label: &'static str, cards: AnyElement, colors: &Colors) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(start::LABEL_GAP)
        .child(
            div()
                .pl(start::LABEL_INSET)
                .text_size(text::LABEL)
                .font_weight(text::LABEL_WEIGHT)
                .text_color(colors.text_dim)
                .child(label),
        )
        .child(cards)
}

pub const FAN_ENTRIES: usize = 3;

pub fn titles(
    name: String,
    subtitle: String,
    name_size: Pixels,
    colors: &Colors,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .min_w_0()
        .gap(start::SUBTITLE_GAP)
        .child(
            div()
                .truncate()
                .text_size(name_size)
                .font_weight(text::STRONG_WEIGHT)
                .text_color(colors.text)
                .child(name),
        )
        .child(
            div()
                .truncate()
                .text_size(text::LABEL)
                .font_weight(text::DETAIL_WEIGHT)
                .text_color(colors.text_dim)
                .child(subtitle),
        )
}

pub fn fan(kinds: impl IntoIterator<Item = EntryKind>, colors: &Colors) -> impl IntoElement {
    let shown: Vec<EntryKind> = kinds.into_iter().take(FAN_ENTRIES).collect();
    let count = shown.len();
    div()
        .flex()
        .flex_none()
        .children(shown.into_iter().enumerate().map(|(index, kind)| {
            let (glyph, tint) = kind_glyph(kind, colors);
            svg()
                .path(glyph.path())
                .flex_none()
                .size(start::FAN_ICON)
                .ml(overlap(index))
                .text_color(Hsla::from(tint))
                .with_transformation(Transformation::rotate(lean(index, count)))
        }))
}

fn overlap(index: usize) -> Pixels {
    if index == 0 {
        Pixels::ZERO
    } else {
        -start::FAN_OVERLAP
    }
}

fn lean(index: usize, count: usize) -> Radians {
    let degrees = if count < FAN_ENTRIES {
        0.0
    } else if index == 0 {
        -start::FAN_TILT
    } else if index + 1 == count {
        start::FAN_TILT
    } else {
        0.0
    };
    Radians(degrees.to_radians())
}

pub fn space_bar(share: Option<f32>, tint: Rgba, colors: &Colors) -> AnyElement {
    div()
        .h(start::BAR)
        .rounded(radius::SMALL)
        .bg(colors.press)
        .overflow_hidden()
        .when_some(share, |bar, share| {
            bar.child(
                div()
                    .h_full()
                    .w(relative(share))
                    .rounded(radius::SMALL)
                    .bg(tint),
            )
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_fan_entry_after_the_first_tucks_under_the_one_before() {
        assert_eq!(overlap(0), Pixels::ZERO);
        assert_eq!(overlap(1), -start::FAN_OVERLAP);
        assert_eq!(overlap(2), -start::FAN_OVERLAP);
    }

    #[test]
    fn a_full_fan_leans_its_outer_entries_out() {
        let tilt = start::FAN_TILT.to_radians();
        assert_eq!(lean(0, 3), Radians(-tilt));
        assert_eq!(lean(1, 3), Radians(0.0));
        assert_eq!(lean(2, 3), Radians(tilt));
        assert_eq!(lean(0, 2), Radians(0.0));
        assert_eq!(lean(1, 2), Radians(0.0));
    }
}
