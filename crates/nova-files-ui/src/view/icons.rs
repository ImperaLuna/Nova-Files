//! Icons: the folder's entries as a grid of icons with their names, at
//! one of five sizes (see `grid`). Like Details it is a `uniform_list`,
//! here of lines of cells, so only the lines on screen are built.
//!
//! A cell does what a row does: a click selects, a double-click opens, a
//! right-click opens the entry's menu, a middle-click opens a folder in a
//! new tab, a folder takes files dropped on it, and the icon and the name
//! drag the selection. A press elsewhere in a cell, or in the space after
//! the last one, drags a box (see `band`). A rename puts its field where
//! the name was. There are no column heads.

use std::ops::Range;

use gpui::{
    AnyElement, Context, Div, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, div,
    prelude::FluentBuilder as _, transparent_black, uniform_list,
};
use nova_files_api::{EntryKind, Listing};

use super::row::{cursor_ring, detail, drags, entry_icon_sized};
use super::sheet::rename_box;
use crate::grid::{CELL_PAD, Grid, NAME_GAP, NAME_LINE, NAME_LINES, NAME_OVERRUN};
use crate::theme::{Colors, opacity, radius, size, text};
use crate::window::{Aim, DraggedRow, MainWindow};

impl MainWindow {
    /// The entries as lines of cells laid out as `grid`.
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

    /// The cell of entry `index`: its icon, and its name beside or under
    /// it.
    fn icon_cell(
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
        let row = (self.pick.is_none() && !self.in_trash()).then(|| DraggedRow::new(at.0, at.1));
        let name = entry.name.to_string_lossy().into_owned();
        let hidden = entry.is_hidden();
        let folder_at = (entry.kind == EntryKind::Folder)
            .then(|| Aim::folder(listing.path_of(entry), name.clone()));
        let picture = drags(
            div()
                .id(("icon", index))
                .flex_none()
                .when(hidden, |slot| slot.opacity(opacity::HIDDEN)),
            row.clone(),
            cx,
        )
        .child(entry_icon_sized(
            self.row_icon(index, entry.kind),
            grid.icon,
            &colors,
        ));
        let label = self.rename_field(index).map_or_else(
            || {
                self.cell_name((index, name), grid, row, cx)
                    .into_any_element()
            },
            |field| rename_box(field.clone(), &colors).into_any_element(),
        );
        cell(index, at, grid, cx)
            .when(hidden, |cell| cell.text_color(colors.text_dim))
            .when_some(folder_at, |cell, at| self.drop_target(cell, at, cx))
            .when(self.selects(at), |cell| {
                cell.bg(colors.selected).text_color(colors.text)
            })
            .child(picture)
            .child(label)
            .when(self.cursor() == Some(index), |cell| {
                cell.child(cursor_ring(&colors))
            })
            .when(self.pick_dims(index), |cell| cell.opacity(opacity::DIMMED))
            .on_click(cx.listener(move |this, event, window, cx| {
                this.row_clicked(at, event, window, cx);
            }))
            .on_aux_click(cx.listener(move |this, event, _, cx| {
                this.entry_aux_clicked(at, event, cx);
            }))
            .into_any_element()
    }

    /// Entry `index`'s name in a cell: on one line beside the icon, over
    /// its size; centred under it on up to two lines.
    fn cell_name(
        &self,
        (index, name): (usize, String),
        grid: Grid,
        row: Option<DraggedRow>,
        cx: &Context<Self>,
    ) -> Div {
        let handle = drags(
            div()
                .id(("name", index))
                .debug_selector(move || format!("name-{index}"))
                .min_w_0()
                .text_size(text::BODY),
            row,
            cx,
        );
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
            // GPUI lets a line cut short overrun its width by about the
            // ellipsis, so the box leaves that much either side.
            handle
                .w_full()
                .px(NAME_OVERRUN)
                .text_center()
                .line_height(NAME_LINE)
                .line_clamp(NAME_LINES)
                .text_ellipsis()
                .child(name),
        )
    }
}

/// Entry `index`'s cell box, `at` in its listing, sized and laid out as
/// `grid` says, highlighted under the pointer. A right-click on it opens
/// its entry's menu (see `window::context_menu`). It keeps the clear edge
/// a folder lights up as a drop target (see `drop`), so all cells line up.
fn cell(
    index: usize,
    at: (nova_files_api::ListingId, nova_files_api::EntryId),
    grid: Grid,
    cx: &Context<MainWindow>,
) -> gpui::Stateful<Div> {
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
        .when(grid.beside, |cell| cell.gap(size::NAME_GAP))
        .when(!grid.beside, |cell| {
            cell.flex_col().justify_start().gap(NAME_GAP)
        })
        .w(grid.cell.width)
        .h(grid.cell.height)
        .p(CELL_PAD - size::DROP_EDGE)
        .border(size::DROP_EDGE)
        .border_color(transparent_black())
        .rounded(radius::ROW)
        .cursor_pointer()
        .hover(|cell| cell.bg(colors.hover))
}
