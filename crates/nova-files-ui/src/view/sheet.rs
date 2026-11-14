//! The sheet the folder's entries sit on: the column heads, then the rows.
//!
//! The rows are a `uniform_list`, which builds only the rows on screen, so
//! a folder of any size costs the same to draw. Each row is an icon and a
//! name, then Modified and Size; those two stay empty until the core has
//! read that entry's details. Selected rows are grey, and selected rows
//! that touch join into one block: a row squares the corners it shares
//! with a selected neighbour. The row the keyboard is on has a ring (see
//! `selecting`). The heads mark the column the list is ordered by: the
//! core lists folders first, each group A to Z.
//!
//! Every row spans the list's full width: `uniform_list` lays each row out
//! on its own, and a row sized to its content would push its columns along
//! by the width of its name.

use gpui::{
    Context, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _,
    uniform_list,
};

use super::row::{Corners, cells, cursor_ring, detail, entry_icon, entry_name, icon};
use crate::assets::Icon;
use crate::theme::{Colors, opacity, radius, size, text};
use crate::window::MainWindow;

const NAME_LABEL: &str = "Name";
const MODIFIED_LABEL: &str = "Modified";
const SIZE_LABEL: &str = "Size";
const MODIFIED_HEADER_SELECTOR: &str = "modified-header";

impl MainWindow {
    pub(super) fn sheet(&self, cx: &Context<Self>) -> impl IntoElement {
        let colors = Colors::of(cx);
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .px(size::SHEET_PAD)
            .bg(colors.sheet)
            .rounded_tl(radius::SHEET)
            .child(column_heads(&colors))
            .child(self.entries_area(cx))
    }

    /// The entries, where a press that isn't on a name can drag a box
    /// (see `band`).
    fn entries_area(&self, cx: &Context<Self>) -> impl IntoElement {
        let colors = Colors::of(cx);
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, _| {
                    let on_name = std::mem::take(&mut this.pressed_name);
                    this.band_pressed(event, on_name);
                }),
            )
            .child(self.entries(cx))
            .children(self.band_box().map(|band| {
                div()
                    .debug_selector(|| "band".to_owned())
                    .absolute()
                    .left(band.origin.x)
                    .top(band.origin.y)
                    .w(band.size.width)
                    .h(band.size.height)
                    .rounded(radius::SMALL)
                    .bg(colors.selected)
                    .border(size::LINE)
                    .border_color(colors.ring)
            }))
    }

    fn entries(&self, cx: &Context<Self>) -> impl IntoElement {
        let count = self
            .listing
            .as_ref()
            .map_or(0, |listing| listing.entries.len());
        uniform_list(
            "entries",
            count,
            cx.processor(|this, range: std::ops::Range<usize>, window, cx| {
                this.rows_drawn(range.clone(), window, cx);
                let Some(listing) = &this.listing else {
                    return Vec::new();
                };
                let listing_id = listing.id;
                let colors = Colors::of(cx);
                let selected = |index: usize| {
                    listing
                        .entries
                        .get(index)
                        .is_some_and(|entry| this.selects((listing_id, entry.id)))
                };
                listing
                    .entries
                    .get(range.clone())
                    .unwrap_or_default()
                    .iter()
                    .zip(range.start..)
                    .map(|(entry, index)| {
                        let id = entry.id;
                        let columns = this.columns(index).unwrap_or_default();
                        let name = SharedString::from(entry.name.to_string_lossy().into_owned());
                        let on = selected(index);
                        let corners = Corners::of(index, selected);
                        let line = cells(
                            div()
                                .flex()
                                .items_center()
                                .gap(size::NAME_GAP)
                                .child(entry_icon(
                                    &entry.name,
                                    entry.kind,
                                    this.thumbnail(index),
                                    &colors,
                                ))
                                .child(entry_name(name, index, cx)),
                            detail(columns.modified, &colors),
                            detail(columns.size, &colors),
                            format!("modified-{index}"),
                        );
                        entry_row(index, corners, &colors)
                            .when(on, |row| row.bg(colors.selected).text_color(colors.text))
                            .child(line)
                            .when(this.cursor() == Some(index), |row| {
                                row.relative().child(cursor_ring(&colors))
                            })
                            .when(this.pick_dims(index), |row| row.opacity(opacity::DIMMED))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, event, window, cx| {
                                this.row_clicked((listing_id, id), event, window, cx);
                            }))
                            .into_any_element()
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full()
    }
}

/// Row `index`'s box, as wide as the list, highlighted under the pointer,
/// with only the corners `corners` keeps rounded.
fn entry_row(index: usize, corners: Corners, colors: &Colors) -> gpui::Stateful<gpui::Div> {
    div()
        .id(("entry", index))
        .debug_selector(move || format!("entry-{index}"))
        .w_full()
        .flex()
        .flex_none()
        .items_center()
        .h(size::ROW_HEIGHT)
        .px(size::ROW_PAD)
        .when(corners.top, |row| {
            row.rounded_tl(radius::ROW).rounded_tr(radius::ROW)
        })
        .when(corners.bottom, |row| {
            row.rounded_bl(radius::ROW).rounded_br(radius::ROW)
        })
        .hover(|row| row.bg(colors.hover))
}

/// The heads above the entries, lined up with [`cells`]. The list is in
/// the core's order, folders first and each group A to Z, so Name carries
/// the arrow.
fn column_heads(colors: &Colors) -> impl IntoElement {
    let head = |label: &'static str| div().flex().items_center().child(label);
    div()
        .flex()
        .flex_none()
        .items_end()
        .h(size::HEAD_HEIGHT)
        .pt(size::HEAD_TOP)
        .mb(size::HEAD_BOTTOM)
        .px(size::ROW_PAD)
        .text_size(text::LABEL)
        .font_weight(text::LABEL_WEIGHT)
        .text_color(colors.text_dim)
        .border_b(size::LINE)
        .border_color(colors.line)
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .h_full()
                .child(cells(
                    head(NAME_LABEL)
                        .gap(size::HEAD_ARROW_GAP)
                        .text_color(colors.text)
                        .child(icon(Icon::SortUp, size::HEAD_ARROW, colors.text)),
                    head(MODIFIED_LABEL),
                    head(SIZE_LABEL).justify_end(),
                    MODIFIED_HEADER_SELECTOR.to_owned(),
                )),
        )
}
