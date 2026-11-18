//! The sheet the folder's entries sit on: the column heads, then the rows;
//! on Start, the Start page instead (see `start`).
//!
//! The rows are a `uniform_list`, which builds only the rows on screen, so
//! a folder of any size costs the same to draw. Each row is an icon and a
//! name, then Modified and Size; those two stay empty until the core has
//! read that entry's details. Selected rows are grey, and selected rows
//! that touch join into one block: a row squares the corners it shares
//! with a selected neighbour. The row the keyboard is on has a ring (see
//! `selecting`); a rename puts its text field where the name was (see
//! `window::renaming`). Middle-click on a folder opens it in a new tab.
//! A click on a column head orders the list by that column;
//! folders always come first (see `navigate` and the core's `order`). A
//! search's files stay best first, so their heads do nothing; they get a
//! Folder column, and the part of each name that matched is marked. The
//! count and the empty look are the search's (see `search`). The
//! trash adds a Was in column, and says so when it is empty (see
//! `window::trash`). A hidden entry's name and icon are dimmed (see
//! `window::hidden`). A right-click on a row or on the empty space below
//! opens its menu (see `window::context_menu`). A name can be dragged; a
//! folder row takes files dropped on it, and the rest of the list takes
//! them into the folder on screen (see `drop` and `window::dragging`).
//!
//! Every row spans the list's full width: `uniform_list` lays each row out
//! on its own, and a row sized to its content would push its columns along
//! by the width of its name.

use std::path::Path;

use gpui::{
    AnyElement, ClickEvent, Context, Div, HitboxBehavior, InteractiveElement as _, IntoElement,
    MouseButton, MouseDownEvent, ParentElement as _, ScrollWheelEvent, Stateful,
    StatefulInteractiveElement as _, Styled as _, canvas, div, prelude::FluentBuilder as _,
    transparent_black, uniform_list,
};

use nova_files_api::{Entry, EntryId, Gathering, Listing, ListingId, Order, SortKey};

use super::drop::inside_edge;
use super::row::{Corners, cells, cursor_ring, detail, entry_icon, entry_name, folder_aim, icon};

use crate::assets::Icon;
use crate::grid::Grid;
use crate::marks;
use crate::theme::{Colors, opacity, radius, size, text};
use crate::window::{Aim, DraggedRow, LIBRARY_LABEL, MainWindow};

const NAME_LABEL: &str = "Name";
const MODIFIED_LABEL: &str = "Modified";
const SIZE_LABEL: &str = "Size";
const WAS_IN_LABEL: &str = "Was in";
const TRASH_EMPTY_LABEL: &str = "Empty";
const FOLDER_LABEL: &str = "Folder";
const MODIFIED_HEADER_SELECTOR: &str = "modified-header";
const FOLDER_HEADER_SELECTOR: &str = "folder-header";

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
            .map(|sheet| {
                if self.is_on_start() {
                    // The page runs to the sheet's edges, glow and all.
                    sheet.px_0().child(self.start_page(cx))
                } else {
                    sheet
                        .when(self.showing_details(), |sheet| {
                            sheet.child(self.column_heads(&colors, cx))
                        })
                        .child(self.entries_area(cx))
                        .child(self.island(cx))
                }
            })
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
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.space_right_clicked(event.position, cx);
                }),
            )
            .when_some(
                self.list_drop_folder()
                    .map(|folder| Aim::folder(folder.to_path_buf(), self.folder_name(folder))),
                |area, at| self.takes_drops(area, at, cx),
            )
            .child(self.entries(cx))
            .child(ctrl_scroll(cx))
            .children(self.empty_trash(&colors))
            .children(self.no_matches(&colors))
            .children(self.match_count(&colors))
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

    fn entries(&self, cx: &Context<Self>) -> AnyElement {
        let grid = self.grid();
        if self.showing_icons() {
            return self.icons(grid, cx).into_any_element();
        }
        if self.showing_gallery() {
            return self.gallery(grid, cx).into_any_element();
        }
        self.details(grid, cx)
    }

    /// Details: a row each, laid out as `grid`.
    fn details(&self, grid: Grid, cx: &Context<Self>) -> AnyElement {
        let count = self
            .listing
            .as_ref()
            .map_or(0, |listing| listing.entries.len());
        uniform_list(
            "entries",
            count,
            cx.processor(move |this, range: std::ops::Range<usize>, window, cx| {
                this.rows_drawn((range.clone(), grid), window, cx);
                let Some(listing) = &this.listing else {
                    return Vec::new();
                };
                let listing_id = listing.id;
                let colors = Colors::of(cx);
                let query = this.searching().then(|| this.query(cx));
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
                        let name = entry.name.to_string_lossy().into_owned();
                        let marks = query
                            .as_deref()
                            .map(|query| marks::matched(&name, query))
                            .unwrap_or_default();
                        let folder = this.folder_cell(listing, entry, index, cx);
                        let on = selected(index);
                        let corners = Corners::of(index, selected);
                        let folder_at = folder_aim(listing, entry, &name);
                        let line = cells(
                            this.name_cell(
                                entry,
                                (DraggedRow::new(listing_id, id), index),
                                (name, marks),
                                cx,
                            ),
                            folder,
                            detail(columns.modified, &colors),
                            detail(columns.size, &colors),
                            format!("modified-{index}"),
                        );
                        entry_row(index, (listing_id, id), corners, &colors, cx)
                            .when_some(folder_at, |row, at| this.drop_target(row, at, cx))
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
                            .on_aux_click(cx.listener(move |this, event, _, cx| {
                                this.entry_aux_clicked((listing_id, id), event, cx);
                            }))
                            .into_any_element()
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full()
        .pb(size::ISLAND_CLEARANCE)
        .into_any_element()
    }
}

impl MainWindow {
    /// The `index`th entry's icon and name, with the parts of the name in
    /// `marks` marked, draggable as `row` outside a picker and the trash;
    /// its rename field in place of the name while it is renamed. A hidden
    /// entry is dimmed.
    fn name_cell(
        &self,
        entry: &Entry,
        (row, index): (DraggedRow, usize),
        (name, marks): (String, Vec<std::ops::Range<usize>>),
        cx: &Context<Self>,
    ) -> Div {
        let draggable = self.entries_drag();
        let colors = &Colors::of(cx);
        let hidden = entry.is_hidden();
        div()
            .flex()
            .items_center()
            .gap(size::NAME_GAP)
            .when(hidden, |line| line.text_color(colors.text_dim))
            .child(
                div()
                    .flex_none()
                    .when(hidden, |slot| slot.opacity(opacity::HIDDEN))
                    .child(entry_icon(self.row_icon(index, entry.kind), colors)),
            )
            .child(self.rename_field(index).map_or_else(
                || {
                    entry_name(name.into(), marks, index, draggable.then_some(row), cx)
                        .into_any_element()
                },
                |field| rename_box(field.clone(), colors).into_any_element(),
            ))
    }

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

/// Takes Ctrl+scroll over the entries before the list scrolls, for the
/// icons' size (see `window::views`). It draws nothing.
fn ctrl_scroll(cx: &Context<MainWindow>) -> impl IntoElement {
    let view = cx.entity().downgrade();
    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |_, hitbox, window, _| {
            window.on_mouse_event(move |event: &ScrollWheelEvent, _, window, cx| {
                if !event.modifiers.secondary() || !hitbox.is_hovered(window) {
                    return;
                }
                let taken = view
                    .update(cx, |this, cx| this.zoom_scrolled(event.delta, cx))
                    .unwrap_or(false);
                if taken {
                    cx.stop_propagation();
                }
            });
        },
    )
    .absolute()
    .inset_0()
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
/// A press in it stays in it: it neither clicks the row nor starts a box.
pub(super) fn rename_box(field: gpui::Entity<crate::text::TextField>, colors: &Colors) -> Div {
    div()
        .debug_selector(|| "rename".to_owned())
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
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
/// with only the corners `corners` keeps rounded. A right-click on it
/// opens its entry's menu (see `window::context_menu`). Every row keeps
/// the clear edge a folder row lights up as a drop target (see `drop`),
/// so all rows line up.
fn entry_row(
    index: usize,
    entry: (ListingId, EntryId),
    corners: Corners,
    colors: &Colors,
    cx: &Context<MainWindow>,
) -> Stateful<Div> {
    div()
        .id(("entry", index))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.entry_right_clicked(entry, event.position, cx);
            }),
        )
        .debug_selector(move || format!("entry-{index}"))
        .w_full()
        .flex()
        .flex_none()
        .items_center()
        .h(size::ROW_HEIGHT)
        .px(inside_edge(size::ROW_PAD))
        .border(size::DROP_EDGE)
        .border_color(transparent_black())
        .when(corners.top, |row| {
            row.rounded_tl(radius::ROW).rounded_tr(radius::ROW)
        })
        .when(corners.bottom, |row| {
            row.rounded_bl(radius::ROW).rounded_br(radius::ROW)
        })
        .hover(|row| row.bg(colors.hover))
}

impl MainWindow {
    /// Where files dropped on the list but not on a folder go: the
    /// folder on screen; nowhere on a search's files, in the trash, or in
    /// Games or Projects.
    pub fn list_drop_folder(&self) -> Option<&Path> {
        if self.searching() || self.trash_open() || self.gathered_shown().is_some() {
            return None;
        }
        Some(&self.listing.as_ref()?.path)
    }

    /// The cell between Name and Modified: where the `index`th entry was
    /// in the trash, a game's library, or the folder of a search's file
    /// or a project; none in a folder.
    fn folder_cell(
        &self,
        listing: &Listing,
        entry: &Entry,
        index: usize,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        if self.trash_open() {
            let was_in = self.was_in(index).unwrap_or_default();
            let colors = Colors::of(cx);
            return Some(detail(was_in.to_string(), &colors).into_any_element());
        }
        if self.gathered_shown() == Some(Gathering::Games) {
            let library = self.library_of(&listing.path_of(entry)).unwrap_or_default();
            let colors = Colors::of(cx);
            return Some(detail(library, &colors).into_any_element());
        }
        listing
            .folders
            .is_some()
            .then(|| self.folder_link(listing.path_of(entry), index, cx))
    }

    /// The heads above the entries, lined up with [`cells`]. A click on
    /// one orders the list by it (see `navigate`); the one in use is
    /// bright and carries an arrow for its direction. A search's files
    /// stay best first: none is in use and a click does nothing, and they
    /// add the Folder head, as Projects does; Games adds Library.
    fn column_heads(&self, colors: &Colors, cx: &Context<Self>) -> impl IntoElement {
        let order = (!self.searching()).then(|| self.shown_order());
        let folder = match self.gathered_shown() {
            _ if self.trash_open() => Some(was_in_head()),
            Some(Gathering::Games) => Some(library_head()),
            Some(Gathering::Projects) => Some(folder_head()),
            None => self.showing_found().then(folder_head),
        };
        column_heads(order, folder, colors, cx)
    }
}

/// The Folder column's head, which orders nothing.
fn folder_head() -> AnyElement {
    div()
        .debug_selector(|| FOLDER_HEADER_SELECTOR.to_owned())
        .child(FOLDER_LABEL)
        .into_any_element()
}

/// Games' Library head, which orders nothing.
fn library_head() -> AnyElement {
    div()
        .debug_selector(|| "head-library".to_owned())
        .child(LIBRARY_LABEL)
        .into_any_element()
}

/// The trash's Was in head, which orders nothing.
fn was_in_head() -> AnyElement {
    div()
        .debug_selector(|| "head-was-in".to_owned())
        .child(WAS_IN_LABEL)
        .into_any_element()
}

/// The heads, ordered by `order` (none in use without one), with the
/// `folder` head between Name and Modified when there is one.
fn column_heads(
    order: Option<Order>,
    folder: Option<AnyElement>,
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
                    folder,
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
    order: Option<Order>,
    colors: &Colors,
    cx: &Context<MainWindow>,
) -> Stateful<Div> {
    let in_use = order.is_some_and(|order| order.key == key);
    let arrow = if order.is_some_and(|order| order.descending) {
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
        .when(order.is_some(), |head| {
            head.cursor_pointer()
                .hover(|head| head.bg(colors.hover).text_color(colors.text))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.sort_by(key);
                    cx.notify();
                }))
        })
        .when(in_use, |head| head.text_color(colors.text))
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
