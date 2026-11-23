use std::ops::Range;

use gpui::{
    AnyElement, Context, Div, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent,
    ParentElement as _, Stateful, StatefulInteractiveElement as _, Styled as _, div,
    prelude::FluentBuilder as _, transparent_black, uniform_list,
};
use nova_files_api::{EntryId, Listing, ListingId};

use super::row::{cursor_ring, detail, drags, entry_icon_sized, folder_aim};
use super::sheet::rename_box;
use crate::theme::{Colors, opacity, radius, size, text};
use crate::window::{DraggedRow, MainWindow};
use nova_files_shell::{Grid, NAME_LINES};

impl MainWindow {
    pub(super) fn icons(&self, grid: Grid, cx: &Context<Self>) -> impl IntoElement {
        let len = self
            .listing
            .as_ref()
            .map_or(0, |listing| listing.entries.len());
        uniform_list(
            "entries",
            grid.lines(len),
            cx.processor(move |this, lines: Range<usize>, window, cx| {
                this.rows_drawn((lines.clone(), grid), window, cx);
                let Some(listing) = &this.listing else {
                    return Vec::new();
                };
                let len = listing.entries.len();
                lines
                    .map(|line| {
                        div()
                            .flex()
                            .w_full()
                            .h(grid.cell.height)
                            .children(
                                grid.entries_of(&(line..line + 1), len)
                                    .map(|index| this.icon_cell(listing, index, grid, cx)),
                            )
                            .into_any_element()
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full()
        .pb(size::ISLAND_CLEARANCE)
    }

    pub(super) fn icon_cell(
        &self,
        listing: &Listing,
        index: usize,
        grid: Grid,
        cx: &Context<Self>,
    ) -> AnyElement {
        let Some(entry) = listing.entries.get(index) else {
            return div().into_any_element();
        };
        let colors = Colors::of(cx);
        let at = (listing.id, entry.id);
        let row = self.entry_drag(at, entry, cx);
        let name = entry.name.to_string_lossy().into_owned();
        let hidden = entry.is_hidden();
        let folder_at = folder_aim(listing, entry, &name);
        let picture = div()
            .debug_selector(move || format!("icon-{index}"))
            .flex_none()
            .when(hidden, |slot| slot.opacity(opacity::HIDDEN))
            .child(
                self.parts
                    .row_icon(entry, grid.icon, cx)
                    .unwrap_or_else(|| {
                        entry_icon_sized(self.row_icon(index, entry.kind), grid.icon, &colors)
                            .into_any_element()
                    }),
            );
        let label = grid.names.then(|| {
            if let Some(cell) = self.parts.name_cell(entry, cx) {
                return div()
                    .when(!grid.beside, |slot| slot.flex_none().w_full())
                    .child(cell)
                    .into_any_element();
            }
            self.parts.name_field(entry, cx).map_or_else(
                || self.cell_name((index, name), grid, cx).into_any_element(),
                |field| {
                    rename_box(field, &colors)
                        .when(!grid.beside, |field| field.flex_none().w_full().ml_0())
                        .into_any_element()
                },
            )
        });
        cell(index, at, grid, cx)
            .when(hidden, |cell| cell.text_color(colors.text_dim))
            .when_some(folder_at, |cell, at| self.drop_target(cell, at, cx))
            .when(self.selects(at), |cell| {
                cell.bg(colors.selected).text_color(colors.text)
            })
            .child(
                item(index, at, grid, row, cx)
                    .child(picture)
                    .children(label),
            )
            .when(self.cursor() == Some(index), |cell| {
                cell.child(cursor_ring(&colors))
            })
            .when(self.dims(index, cx), |cell| cell.opacity(opacity::DIMMED))
            .on_click(cx.listener(move |this, event, window, cx| {
                this.row_clicked(at, event, window, cx);
            }))
            .on_aux_click(cx.listener(move |this, event, _, cx| {
                this.entry_aux_clicked(at, event, cx);
            }))
            .into_any_element()
    }

    fn cell_name(&self, (index, name): (usize, String), grid: Grid, cx: &Context<Self>) -> Div {
        let handle = div()
            .debug_selector(move || format!("name-{index}"))
            .min_w_0()
            .text_size(text::BODY);
        if grid.beside {
            let size_text = self
                .columns(index)
                .map(|columns| columns.size)
                .unwrap_or_default();
            return div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(handle.truncate().child(name))
                .child(detail(size_text, &Colors::of(cx)));
        }
        div().w_full().child(
            handle
                .w_full()
                .px(size::ICON_NAME_OVERRUN)
                .text_center()
                .line_height(size::ICON_NAME_LINE)
                .line_clamp(NAME_LINES)
                .text_ellipsis()
                .child(name),
        )
    }
}

fn item(
    index: usize,
    at: (ListingId, EntryId),
    grid: Grid,
    row: Option<DraggedRow>,
    cx: &Context<MainWindow>,
) -> Stateful<Div> {
    drags(div().id(("item", index)), at, row, cx)
        .flex()
        .w_full()
        .items_center()
        .when(grid.beside, |item| item.gap(size::NAME_GAP))
        .when(!grid.beside, |item| {
            item.flex_col().justify_start().gap(size::ICON_NAME_GAP)
        })
}

fn cell(
    index: usize,
    at: (ListingId, EntryId),
    grid: Grid,
    cx: &Context<MainWindow>,
) -> Stateful<Div> {
    let colors = Colors::of(cx);
    div()
        .id(("entry", index))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.entry_right_clicked(at, event.position, cx);
            }),
        )
        .debug_selector(move || format!("entry-{index}"))
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .when(!grid.beside, |cell| cell.flex_col().justify_start())
        .m(size::ICON_CELL_APART)
        .w(grid.cell.width - size::ICON_CELL_APART - size::ICON_CELL_APART)
        .h(grid.cell.height - size::ICON_CELL_APART - size::ICON_CELL_APART)
        .p(size::ICON_CELL_PAD - size::DROP_EDGE - size::ICON_CELL_APART)
        .border(size::DROP_EDGE)
        .border_color(transparent_black())
        .rounded(radius::ROW)
        .cursor_pointer()
        .hover(|cell| cell.bg(colors.hover))
}
