//! The sheet the folder's entries sit on: the column heads, then the rows.
//!
//! The rows are a `uniform_list`, which builds only the rows on screen, so
//! a folder of any size costs the same to draw. Each row is an icon and a
//! name, then Modified and Size; those two stay empty until the core has
//! read that entry's details. Selected rows are grey, and selected rows
//! that touch join into one block: a row squares the corners it shares
//! with a selected neighbour. The row the keyboard is on has a ring (see
//! `selecting`); a rename puts its text field where the name was (see
//! `window::renaming`). A click on a column head orders the list by that column;
//! folders always come first (see `navigate` and the core's `order`). The
//! trash adds a Was in column, and says so when it is empty (see
//! `window::trash`).
//!
//! Every row spans the list's full width: `uniform_list` lays each row out
//! on its own, and a row sized to its content would push its columns along
//! by the width of its name.

use gpui::{
    ClickEvent, Context, Div, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent,
    ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, div,
    prelude::FluentBuilder as _, uniform_list,
};

use nova_files_api::{Order, SortKey};

use super::row::{Corners, cells, cursor_ring, detail, entry_icon, entry_name, icon};

use crate::assets::Icon;
use crate::theme::{Colors, opacity, radius, size, text};
use crate::window::MainWindow;

const NAME_LABEL: &str = "Name";
const MODIFIED_LABEL: &str = "Modified";
const SIZE_LABEL: &str = "Size";
const WAS_IN_LABEL: &str = "Was in";
const TRASH_EMPTY_LABEL: &str = "Empty";
const MODIFIED_HEADER_SELECTOR: &str = "modified-header";

impl MainWindow {
    pub(super) fn sheet(&self, cx: &Context<Self>) -> impl IntoElement {
        let colors = Colors::of(cx);
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .relative()
            .px(size::SHEET_PAD)
            .child(sheet_corner(&colors))
            .child(column_heads(
                self.shown_order(),
                self.trash_open(),
                &colors,
                cx,
            ))
            .child(self.entries_area(cx))
            .child(self.island(cx))
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
            .children(self.empty_trash(&colors))
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
                let in_trash = this.trash_open();
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
                                .child(this.rename_field(index).map_or_else(
                                    || entry_name(name, index, cx).into_any_element(),
                                    |field| rename_box(field.clone(), &colors).into_any_element(),
                                )),
                            in_trash.then(|| {
                                let was_in = this.was_in(index).unwrap_or_default();
                                detail(was_in.to_string(), &colors).into_any_element()
                            }),
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
        .pb(size::ISLAND_CLEARANCE)
    }
}

impl MainWindow {
    /// "Empty" in the middle of an empty trash.
    fn empty_trash(&self, colors: &Colors) -> Option<impl IntoElement> {
        let empty = self
            .listing
            .as_ref()
            .is_some_and(|listing| listing.entries.is_empty());
        (empty && self.trash_open()).then(|| {
            div()
                .debug_selector(|| "trash-empty".to_owned())
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(size::NAME_GAP)
                .text_color(colors.text_dim)
                .child(icon(Icon::Trash, size::EMPTY_ICON, colors.text_dim))
                .child(TRASH_EMPTY_LABEL)
        })
    }
}

/// The sheet's rounded corner where the sidebar and bar meet. The sheet
/// itself is the window's fill (see `view`), so only this corner paints
/// the window's color behind a rounded patch of the sheet's.
fn sheet_corner(colors: &Colors) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size(radius::SHEET)
        .bg(colors.window)
        .child(div().size_full().rounded_tl(radius::SHEET).bg(colors.sheet))
}

/// The rename field over an entry's name, its text where the name was.
fn rename_box(field: gpui::Entity<crate::text::TextField>, colors: &Colors) -> impl IntoElement {
    div()
        .debug_selector(|| "rename".to_owned())
        .flex()
        .flex_1()
        .items_center()
        .min_w_0()
        .h(size::RENAME_HEIGHT)
        .px(size::RENAME_PAD)
        .ml(-size::RENAME_PAD)
        .rounded(radius::SMALL)
        .bg(colors.window)
        .border(size::RING)
        .border_color(colors.accept)
        .text_color(colors.text)
        .child(field)
}

/// Row `index`'s box, as wide as the list, highlighted under the pointer,
/// with only the corners `corners` keeps rounded.
fn entry_row(index: usize, corners: Corners, colors: &Colors) -> Stateful<Div> {
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

/// The heads above the entries, lined up with [`cells`]. A click on one
/// orders the list by it (see `navigate`); the one in use is bright and
/// carries an arrow for its direction.
fn column_heads(
    order: Order,
    was_in: bool,
    colors: &Colors,
    cx: &Context<MainWindow>,
) -> impl IntoElement {
    let head = |key, label| column_head(key, label, order, colors, cx);
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
                    head(SortKey::Name, NAME_LABEL),
                    was_in.then(|| {
                        div()
                            .debug_selector(|| "head-was-in".to_owned())
                            .child(WAS_IN_LABEL)
                            .into_any_element()
                    }),
                    head(SortKey::Modified, MODIFIED_LABEL),
                    head(SortKey::Size, SIZE_LABEL).mr(-size::HEAD_BUTTON_PAD),
                    MODIFIED_HEADER_SELECTOR.to_owned(),
                )),
        )
}

/// The head of `key`'s column, with selector `head-<label>`.
fn column_head(
    key: SortKey,
    label: &'static str,
    order: Order,
    colors: &Colors,
    cx: &Context<MainWindow>,
) -> Stateful<Div> {
    let in_use = order.key == key;
    let arrow = if order.descending {
        Icon::SortDown
    } else {
        Icon::SortUp
    };
    let selector = format!("head-{}", label.to_lowercase());
    div()
        .id(label)
        .debug_selector(move || selector)
        .flex()
        .flex_none()
        .items_center()
        .gap(size::HEAD_ARROW_GAP)
        .h(size::HEAD_BUTTON_HEIGHT)
        .px(size::HEAD_BUTTON_PAD)
        .ml(-size::HEAD_BUTTON_PAD)
        .rounded(radius::SMALL)
        .cursor_pointer()
        .hover(|head| head.bg(colors.hover).text_color(colors.text))
        .when(in_use, |head| head.text_color(colors.text))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.sort_by(key);
            cx.notify();
        }))
        .child(label)
        .when(in_use, |head| {
            let arrow_selector = format!("arrow-{}", label.to_lowercase());
            head.child(div().debug_selector(move || arrow_selector).child(icon(
                arrow,
                size::HEAD_ARROW,
                colors.text,
            )))
        })
}
