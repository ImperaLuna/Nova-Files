pub use nova_files_shell::kind::entry_kind;
pub use nova_files_shell::widgets::{detail, icon, icon_slot, kind_icon};
use std::ops::Range;
use std::sync::Arc;

use gpui::{
    AnyElement, App, Context, Div, HighlightStyle, InteractiveElement as _, IntoElement,
    MouseButton, MouseDownEvent, ObjectFit, ParentElement as _, Pixels, RenderImage, SharedString,
    Stateful, StatefulInteractiveElement as _, Styled as _, StyledImage as _, StyledText, div, img,
    prelude::FluentBuilder as _, relative,
};
use nova_files_api::{Entry, EntryId, EntryKind, IconKind, Listing, ListingId};
use nova_files_shell::Allow;

use super::full_name::full_name_on_hover;
use crate::theme::{Colors, radius, size, space};
use crate::window::{Aim, DraggedRow, MainWindow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Corners {
    pub top: bool,
    pub bottom: bool,
}

impl Corners {
    pub fn of(index: usize, selected: impl Fn(usize) -> bool) -> Self {
        let joined = selected(index);
        Self {
            top: !(joined && index.checked_sub(1).is_some_and(&selected)),
            bottom: !(joined && index.checked_add(1).is_some_and(&selected)),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ColumnWidths {
    pub part: Pixels,
    pub folder: Pixels,
    pub modified: Pixels,
    pub size: Pixels,
}

#[derive(Default)]
pub struct Middle {
    pub part: Option<AnyElement>,
    pub folder: Option<AnyElement>,
}

pub fn cells(
    name: impl IntoElement,
    middle: Middle,
    modified: impl IntoElement,
    (size_cell, widths): (impl IntoElement, ColumnWidths),
    modified_selector: String,
) -> impl IntoElement {
    let column = |width: Pixels| {
        div()
            .flex()
            .flex_none()
            .w(width + size::COLUMN_BLEED + size::COLUMN_BLEED)
            .mx(-size::COLUMN_BLEED)
            .px(size::COLUMN_BLEED)
            .min_w_0()
            .truncate()
    };
    div()
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .gap(size::COLUMN_GAP)
        .h_full()
        .child(div().flex_1().min_w_0().h_full().child(name))
        .children(middle.part.map(|part| column(widths.part).child(part)))
        .children(
            middle
                .folder
                .map(|folder| column(widths.folder).child(folder)),
        )
        .child(
            column(widths.modified)
                .debug_selector(|| modified_selector)
                .child(modified),
        )
        .child(column(widths.size).justify_end().child(size_cell))
}

pub fn share_cell(text_content: String, share: Option<f32>, colors: &Colors) -> impl IntoElement {
    let fill = share.map(|share| share.clamp(space::MIN_FILL, 1.0));
    let largest = share.is_some_and(|share| share >= 1.0);
    div()
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .justify_between()
        .gap(space::BAR_GAP)
        .child(
            div()
                .flex_none()
                .w(space::BAR_WIDTH)
                .h(space::BAR_HEIGHT)
                .rounded(radius::SHARE_BAR)
                .bg(colors.press)
                .children(fill.map(|fill| {
                    div()
                        .h_full()
                        .w(relative(fill))
                        .rounded(radius::SHARE_BAR)
                        .bg(if largest {
                            colors.here
                        } else {
                            colors.text_dim
                        })
                })),
        )
        .child(detail(text_content, colors))
}

pub fn entry_name(name: SharedString, marks: Vec<Range<usize>>, index: usize, cx: &App) -> Div {
    let mark = mark_style(&Colors::of(cx));
    let text =
        StyledText::new(name.clone()).with_highlights(marks.into_iter().map(|range| (range, mark)));
    div()
        .debug_selector(move || format!("name-{index}"))
        .min_w_0()
        .truncate()
        .child(full_name_on_hover(("name", index), text, name))
}

pub fn drags(
    handle: Stateful<Div>,
    at: (ListingId, EntryId),
    row: Option<DraggedRow>,
    cx: &Context<MainWindow>,
) -> Stateful<Div> {
    let main = cx.entity();
    handle
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, _| this.pressed_name = true),
        )
        .on_click(cx.listener(move |this, event, window, cx| {
            cx.stop_propagation();
            this.row_clicked(at, event, window, cx);
        }))
        .when_some(row, |handle, row| {
            handle
                .on_drag(row, move |row, _, window, cx| {
                    main.update(cx, |this, cx| this.drag_started(row, window, cx))
                })
                .external_drag_payload(|row: &DraggedRow, _, _| row.drag().outward())
        })
}

impl MainWindow {
    pub(super) fn entry_drag(
        &self,
        (listing, id): (ListingId, EntryId),
        entry: &Entry,
        cx: &App,
    ) -> Option<DraggedRow> {
        let drags = self.lifted()
            && !self.picker()
            && self.folder_rules().allows(Allow::Change)
            && self.parts.name_field(entry, cx).is_none();
        drags.then(|| DraggedRow::new(listing, id))
    }
}

pub fn folder_aim(listing: &Listing, entry: &Entry, name: &str) -> Option<Aim> {
    (entry.kind == EntryKind::Folder).then(|| Aim::folder(listing.path_of(entry), name.to_owned()))
}

fn mark_style(colors: &Colors) -> HighlightStyle {
    HighlightStyle {
        color: Some(colors.text.into()),
        background_color: Some(colors.mark.into()),
        ..HighlightStyle::default()
    }
}

pub fn cursor_ring(colors: &Colors) -> impl IntoElement {
    div()
        .debug_selector(|| "cursor".to_owned())
        .absolute()
        .inset_0()
        .rounded(radius::ROW)
        .border(size::RING)
        .border_color(colors.ring)
}

#[derive(Debug, Clone, Copy)]
pub enum RowIcon<'a> {
    Thumbnail(&'a Arc<RenderImage>),
    Theme(&'a Arc<RenderImage>),
    BuiltIn(IconKind),
}

pub fn entry_icon(shown: RowIcon<'_>, colors: &Colors) -> Div {
    entry_icon_sized(shown, size::ROW_ICON, colors)
}

pub fn entry_icon_sized(shown: RowIcon<'_>, side: Pixels, colors: &Colors) -> Div {
    let slot = icon_slot(side);
    match shown {
        RowIcon::Thumbnail(image) => slot.child(
            img(Arc::clone(image))
                .size(side)
                .object_fit(ObjectFit::Contain)
                .rounded(radius::SMALL),
        ),
        RowIcon::Theme(image) => slot.child(img(Arc::clone(image)).size(side)),
        RowIcon::BuiltIn(kind) => kind_icon(kind, side, colors),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROUND: Corners = Corners {
        top: true,
        bottom: true,
    };

    #[test]
    fn a_match_is_full_strength_text_on_orange() {
        let colors = Colors::default();
        let mark = mark_style(&colors);
        assert_eq!(mark.color, Some(colors.text.into()));
        assert_eq!(mark.background_color, Some(colors.mark.into()));
    }

    #[test]
    fn touching_selected_rows_join_into_one_block() {
        let selected = |index| matches!(index, 0..=2 | 4);
        let corners = |index| Corners::of(index, selected);
        assert_eq!(
            corners(0),
            Corners {
                top: true,
                bottom: false
            }
        );
        assert_eq!(
            corners(1),
            Corners {
                top: false,
                bottom: false
            }
        );
        assert_eq!(
            corners(2),
            Corners {
                top: false,
                bottom: true
            }
        );
        assert_eq!(corners(3), ROUND);
        assert_eq!(corners(4), ROUND);
    }

    #[test]
    fn a_row_not_selected_stays_round_between_selected_ones() {
        let selected = |index| index != 1;
        assert_eq!(Corners::of(1, selected), ROUND);
        assert!(Corners::of(usize::MAX, |_| true).bottom);
    }
}
