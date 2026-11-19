use gpui::{
    Context, Div, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Stateful,
    Styled as _, div, prelude::FluentBuilder as _,
};

use nova_files_shell::{SidebarAt, SidebarRow};

use super::part_rows::expander_button;
use super::{icon, inside_edge, section_head};
use crate::theme::{Colors, radius, size, text};
use crate::window::MainWindow;

const LABEL: &str = "This computer";

impl MainWindow {
    pub(super) fn tree_section(&self, cx: &Context<Self>) -> Div {
        let colors = Colors::of(cx);
        let mut ranked = self.part_rows(SidebarAt::Tree, false, &colors, cx);
        ranked.sort_by_key(|(rank, _)| *rank);
        div()
            .flex()
            .flex_col()
            .child(section_head(
                LABEL,
                self.part_buttons(SidebarAt::Tree, &colors, cx),
                &colors,
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(size::PLACE_GAP)
                    .children(ranked.into_iter().map(|(_, row)| row)),
            )
    }

    pub(super) fn tree_row(
        &self,
        place: SidebarRow,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let depth = place.depth;
        let selector = place.id.to_string();
        let tint = if place.here {
            colors.here
        } else {
            colors.text_dim
        };
        let guides: Vec<Div> = (1..=depth)
            .map(|level| {
                let guide = format!("{selector}-guide-{level}");
                div()
                    .debug_selector(move || guide)
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(guide_left(level))
                    .w(size::LINE)
                    .bg(colors.line)
            })
            .collect();
        let toggle = place.expander.clone().map_or_else(
            || toggle_slot().into_any_element(),
            |expander| expander_button(&place.id, expander, colors, cx),
        );
        let shown = div()
            .id(place.id.clone())
            .debug_selector(move || selector)
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .w_full()
            .h(size::TREE_ROW_HEIGHT)
            .gap(size::TREE_ROW_GAP)
            .pl(inside_edge(indent(depth) + size::TREE_PAD_LEFT))
            .pr(inside_edge(size::TREE_PAD_RIGHT))
            .rounded(radius::ROW)
            .cursor_pointer()
            .whitespace_nowrap()
            .hover(|row| row.bg(colors.hover))
            .when(place.here, |row| {
                row.text_color(colors.text).font_weight(text::STRONG_WEIGHT)
            })
            .children(guides)
            .child(toggle)
            .child(icon(place.icon, size::TREE_ICON, tint))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(place.name.clone()),
            );
        self.part_row_acts(shown, place, cx)
    }
}

fn toggle_slot() -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(size::TREE_TOGGLE)
        .rounded(radius::SMALL)
}

fn indent(depth: u8) -> Pixels {
    size::TREE_INDENT * f32::from(depth)
}

fn guide_left(level: u8) -> Pixels {
    indent(level) - size::TREE_GUIDE_INSET - size::DROP_EDGE
}
