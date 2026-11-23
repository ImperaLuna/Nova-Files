use std::path::Path;

use gpui::{
    AnyElement, ClickEvent, Context, Div, HitboxBehavior, InteractiveElement as _, IntoElement,
    MouseButton, MouseDownEvent, ParentElement as _, Pixels, ScrollEvent, Stateful,
    StatefulInteractiveElement as _, Styled as _, Window, canvas, div, prelude::FluentBuilder as _,
    transparent_black, uniform_list,
};

use nova_files_api::{Entry, EntryId, Listing, ListingId, Order, SortKey};

use super::drop::inside_edge;
use super::row::{
    Corners, cells, cursor_ring, detail, drags, entry_icon, entry_name, folder_aim, icon,
    share_cell,
};

use nova_files_shell::{Allow, PageRef, ShownColumn};

use crate::assets::Icon;
use crate::marks;
use crate::theme::{Colors, opacity, radius, size, space, text};
use crate::window::{Aim, MainWindow};
use nova_files_shell::Grid;

const NAME_LABEL: &str = "Name";
const MODIFIED_LABEL: &str = "Modified";
const SIZE_LABEL: &str = "Size";
const FOLDER_LABEL: &str = "Folder";
const MODIFIED_HEADER_SELECTOR: &str = "modified-header";
const FOLDER_HEADER_SELECTOR: &str = "folder-header";

impl MainWindow {
    pub(super) fn sheet(
        &self,
        entries: Option<AnyElement>,
        cx: &Context<Self>,
    ) -> impl IntoElement {
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
                    sheet.px_0().child(self.start_page(cx))
                } else if let Some(page) = self.page_shown() {
                    sheet
                        .px_0()
                        .children(self.parts.page(page))
                        .child(self.island(cx))
                } else {
                    sheet
                        .when(self.showing_details(cx), |sheet| {
                            sheet.child(self.column_heads(&colors, cx))
                        })
                        .child(self.entries_area(entries, cx))
                        .child(self.island(cx))
                }
            })
    }

    fn entries_area(&self, entries: Option<AnyElement>, cx: &Context<Self>) -> impl IntoElement {
        let colors = Colors::of(cx);
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    let on_name = std::mem::take(&mut this.pressed_name);
                    this.band_pressed(event, on_name, cx);
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
                    .map(|folder| Aim::folder(folder.to_path_buf(), self.folder_name(folder, cx))),
                |area, at| self.takes_drops(area, at, cx),
            )
            .children(entries)
            .children(self.parts.overlays())
            .children(
                (self.listing.is_some() && !self.showing_gallery(cx))
                    .then(|| self.scroll_bar(cx))
                    .flatten(),
            )
            .child(ctrl_scroll(cx))
            .children(self.empty_note(&colors, cx))
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

    pub(super) fn entries(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if let Some(view) = self.parts.list_view(&self.scene(cx), cx) {
            return view.into_any_element();
        }
        let grid = self.grid(cx);
        if self.showing_icons(cx) {
            return self.icons(grid, cx).into_any_element();
        }
        if self.showing_gallery(cx) {
            return self.gallery(grid, window, cx).into_any_element();
        }
        self.details(grid, cx)
    }

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
                let query = this.search.marks().map(str::to_owned);
                let part_column = this.parts.column(&this.scene(cx), cx);
                let size_width = size_width(this.shows_shares());
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
                        let folder =
                            this.folder_cell(listing, (entry, index), part_column.as_ref(), cx);
                        let on = selected(index);
                        let corners = Corners::of(index, selected);
                        let folder_at = folder_aim(listing, entry, &name);
                        let line = cells(
                            this.name_cell(entry, ((listing_id, id), index), (name, marks), cx),
                            folder,
                            detail(columns.modified, &colors),
                            (this.size_cell(index, columns.size, &colors), size_width),
                            format!("modified-{index}"),
                        );
                        entry_row(index, (listing_id, id), corners, &colors, cx)
                            .when_some(folder_at, |row, at| this.drop_target(row, at, cx))
                            .when(on, |row| row.bg(colors.selected).text_color(colors.text))
                            .child(line)
                            .when(this.cursor() == Some(index), |row| {
                                row.relative().child(cursor_ring(&colors))
                            })
                            .when(this.dims(index, cx), |row| row.opacity(opacity::DIMMED))
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
    fn name_cell(
        &self,
        entry: &Entry,
        (at, index): ((ListingId, EntryId), usize),
        (name, marks): (String, Vec<std::ops::Range<usize>>),
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let colors = &Colors::of(cx);
        let hidden = entry.is_hidden();
        let row = self.entry_drag(at, entry, cx);
        drags(div().id(("name-cell", index)), at, row, cx)
            .h_full()
            .flex()
            .items_center()
            .gap(size::NAME_GAP)
            .when(hidden, |line| line.text_color(colors.text_dim))
            .child(
                div()
                    .flex_none()
                    .when(hidden, |slot| slot.opacity(opacity::HIDDEN))
                    .child(
                        self.parts
                            .row_icon(entry, size::ROW_ICON, cx)
                            .unwrap_or_else(|| {
                                entry_icon(self.row_icon(index, entry.kind), colors)
                                    .into_any_element()
                            }),
                    ),
            )
            .child(self.parts.name_cell(entry, cx).unwrap_or_else(|| {
                self.parts.name_field(entry, cx).map_or_else(
                    || entry_name(name.into(), marks, index, cx).into_any_element(),
                    |field| rename_box(field, colors).into_any_element(),
                )
            }))
    }

    fn empty_note(&self, colors: &Colors, cx: &Context<Self>) -> Option<impl IntoElement> {
        let empty = self
            .listing
            .as_ref()
            .is_some_and(|listing| listing.entries.is_empty());
        if !empty {
            return None;
        }
        let (glyph, note) = self.parts.empty_note(&self.scene(cx), cx)?;
        Some(
            div()
                .debug_selector(|| "empty-note".to_owned())
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(size::NAME_GAP)
                .text_color(colors.text_dim)
                .child(icon(glyph, size::EMPTY_ICON, colors.text_dim))
                .child(note),
        )
    }
}

fn ctrl_scroll(cx: &Context<MainWindow>) -> impl IntoElement {
    let view = cx.entity().downgrade();
    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |_, hitbox, window, _| {
            window.on_mouse_event(move |event: &ScrollEvent, _, window, cx| {
                if !event.modifiers.control || !hitbox.is_hovered(window) {
                    return;
                }
                let taken = view
                    .update(cx, |this, cx| this.parts.wheel(event, cx))
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

fn sheet_corner(colors: &Colors) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size(radius::SHEET)
        .bg(colors.window)
        .child(div().size_full().rounded_tl(radius::SHEET).bg(colors.sheet))
}

pub(super) fn rename_box(
    field: gpui::Entity<nova_files_shell::text::TextField>,
    colors: &Colors,
) -> Div {
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
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                this.entry_pressed(entry, event.modifiers, cx);
            }),
        )
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
    pub fn list_drop_folder(&self) -> Option<&Path> {
        if self.searching()
            || !self.folder_rules().allows(Allow::Change)
            || self.listed_page().is_some()
        {
            return None;
        }
        Some(&self.listing.as_ref()?.path)
    }

    fn folder_cell(
        &self,
        listing: &Listing,
        (entry, index): (&Entry, usize),
        part_column: Option<&ShownColumn>,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        if let Some(column) = part_column {
            let text = column.cell(entry, cx).unwrap_or_default();
            return Some(detail(text.to_string(), &Colors::of(cx)).into_any_element());
        }
        listing
            .folders
            .is_some()
            .then(|| self.folder_link(listing.path_of(entry), index, cx))
    }

    fn size_cell(&self, index: usize, text: String, colors: &Colors) -> AnyElement {
        if self.shows_shares() {
            share_cell(text, self.share(index), colors).into_any_element()
        } else {
            detail(text, colors).into_any_element()
        }
    }

    fn column_heads(&self, colors: &Colors, cx: &Context<Self>) -> impl IntoElement {
        let sorted = self.listed_page().is_none_or(PageRef::sorted);
        let order = (!self.searching() && sorted).then(|| self.shown_order());
        let part = self.parts.column(&self.scene(cx), cx).map(|column| {
            let selector = column.selector;
            div()
                .debug_selector(move || selector.to_owned())
                .child(column.head)
                .into_any_element()
        });
        let folder = part.or_else(|| {
            let listed = self.listed_page().is_some_and(PageRef::folder_column);
            (self.showing_found() || listed).then(folder_head)
        });
        column_heads(order, folder, size_width(self.shows_shares()), colors, cx)
    }
}

const fn size_width(shares: bool) -> Pixels {
    if shares {
        space::COLUMN_WIDTH
    } else {
        size::SIZE_WIDTH
    }
}

fn folder_head() -> AnyElement {
    div()
        .debug_selector(|| FOLDER_HEADER_SELECTOR.to_owned())
        .child(FOLDER_LABEL)
        .into_any_element()
}

fn column_heads(
    order: Option<Order>,
    folder: Option<AnyElement>,
    size_width: Pixels,
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
                    div().flex().child(head(SortKey::Name, NAME_LABEL)),
                    folder,
                    head(SortKey::Modified, MODIFIED_LABEL),
                    (
                        head(SortKey::Size, SIZE_LABEL)
                            .justify_end()
                            .mr(-size::HEAD_BUTTON_PAD),
                        size_width,
                    ),
                    MODIFIED_HEADER_SELECTOR.to_owned(),
                )),
        )
}

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
        .flex_1()
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
