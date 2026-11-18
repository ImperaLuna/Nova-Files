//! The pieces of one row in the sheet, shared by the column heads and the
//! entries so their columns line up: the cells, an entry's icon and name,
//! and the ring around the row the keyboard is on. Icons are drawn here
//! for every part of the window. An entry's icon is its thumbnail, else
//! its type's icon from the system icon theme, else the window's own icon
//! for its kind.

use std::ops::Range;
use std::sync::Arc;

use gpui::{
    AnyElement, Context, Div, HighlightStyle, Hsla, InteractiveElement as _, IntoElement,
    MouseButton, MouseDownEvent, ObjectFit, ParentElement as _, Pixels, RenderImage, Rgba,
    SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, StyledImage as _,
    StyledText, div, img, prelude::FluentBuilder as _, svg,
};
use nova_files_api::{EntryKind, IconKind};

use crate::assets::Icon;
use crate::theme::{Colors, radius, size, text};
use crate::window::{DraggedRow, MainWindow};

/// Which corners of a row stay rounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Corners {
    pub top: bool,
    pub bottom: bool,
}

impl Corners {
    /// Row `index`'s corners when `selected` says which rows are
    /// selected: a selected row squares the corners it shares with a
    /// selected neighbour, so touching rows join into one block.
    pub fn of(index: usize, selected: impl Fn(usize) -> bool) -> Self {
        let joined = selected(index);
        Self {
            top: !(joined && index.checked_sub(1).is_some_and(&selected)),
            bottom: !(joined && index.checked_add(1).is_some_and(&selected)),
        }
    }
}

/// One line of the columns: the name takes what the others leave, then
/// the Folder column while a search's files are on screen (in the trash,
/// Was in), and Size sits flush right. The Modified cell carries
/// `modified_selector`. Modified and Size are drawn as details: smaller
/// and dimmer.
pub fn cells(
    name: impl IntoElement,
    folder: Option<AnyElement>,
    modified: impl IntoElement,
    size_cell: impl IntoElement,
    modified_selector: String,
) -> impl IntoElement {
    let column = |width| div().flex().flex_none().w(width).min_w_0().truncate();
    div()
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .gap(size::COLUMN_GAP)
        .child(div().flex_1().min_w_0().child(name))
        .children(folder.map(|folder| column(size::FOLDER_WIDTH).child(folder)))
        .child(
            column(size::MODIFIED_WIDTH)
                .debug_selector(|| modified_selector)
                .child(modified),
        )
        .child(column(size::SIZE_WIDTH).justify_end().child(size_cell))
}

/// An entry's Modified or Size text, as a detail.
pub fn detail(text_content: String, colors: &Colors) -> impl IntoElement {
    div()
        .text_size(text::DETAIL)
        .font_weight(text::DETAIL_WEIGHT)
        .text_color(colors.text_dim)
        .truncate()
        .child(text_content)
}

/// An entry's name, the parts in `marks` (byte ranges) marked as a
/// search's match. A press on it is left for dragging files, so it marks
/// itself for `band`; when it carries `row`, moving the press drags the
/// selection (see `window::dragging`).
pub fn entry_name(
    name: SharedString,
    marks: Vec<Range<usize>>,
    index: usize,
    row: Option<DraggedRow>,
    cx: &Context<MainWindow>,
) -> impl IntoElement {
    let mark = mark_style(&Colors::of(cx));
    let text = StyledText::new(name).with_highlights(marks.into_iter().map(|range| (range, mark)));
    drags(
        div()
            .id(("name", index))
            .debug_selector(move || format!("name-{index}"))
            .min_w_0()
            .truncate(),
        row,
        cx,
    )
    .child(text)
}

/// `handle`, where a press is left for dragging files: it marks itself
/// for `band`, and when it carries `row`, moving the press drags the
/// selection (see `window::dragging`).
pub fn drags(
    handle: Stateful<Div>,
    row: Option<DraggedRow>,
    cx: &Context<MainWindow>,
) -> Stateful<Div> {
    let window = cx.entity();
    handle
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, _| this.pressed_name = true),
        )
        .when_some(row, |handle, row| {
            handle.on_drag(row, move |row, _, _, cx| {
                window.update(cx, |this, cx| this.drag_started(row, cx))
            })
        })
}

/// How a search's match shows in a name: full-strength text on orange.
fn mark_style(colors: &Colors) -> HighlightStyle {
    HighlightStyle {
        color: Some(colors.text.into()),
        background_color: Some(colors.mark.into()),
        ..HighlightStyle::default()
    }
}

/// The ring around the row the keyboard is on, laid over the row so it
/// shifts nothing.
pub fn cursor_ring(colors: &Colors) -> impl IntoElement {
    div()
        .absolute()
        .inset_0()
        .rounded(radius::ROW)
        .border(size::RING)
        .border_color(colors.ring)
}

/// What an entry's icon slot shows.
#[derive(Debug, Clone, Copy)]
pub enum RowIcon<'a> {
    Thumbnail(&'a Arc<RenderImage>),
    /// Its type's icon from the system icon theme, in its own colors.
    Theme(&'a Arc<RenderImage>),
    /// The window's own icon for its kind.
    BuiltIn(IconKind),
}

/// The icon slot of an entry row, the same size whatever it shows, so
/// nothing shifts when a thumbnail or theme icon arrives.
pub fn entry_icon(shown: RowIcon<'_>, colors: &Colors) -> Div {
    entry_icon_sized(shown, size::ROW_ICON, colors)
}

/// An entry's icon slot `side` across, as [`entry_icon`]. A thumbnail
/// keeps its shape inside it.
pub fn entry_icon_sized(shown: RowIcon<'_>, side: Pixels, colors: &Colors) -> Div {
    let slot = div()
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(side);
    match shown {
        RowIcon::Thumbnail(image) => slot.child(
            img(Arc::clone(image))
                .size(side)
                .object_fit(ObjectFit::Contain)
                .rounded(radius::SMALL),
        ),
        RowIcon::Theme(image) => slot.child(img(Arc::clone(image)).size(side)),
        RowIcon::BuiltIn(kind) => {
            let (glyph, tint) = built_in(kind, colors);
            slot.child(icon(glyph, side, tint))
        }
    }
}

/// The kind of icon an entry of `kind` has before the core says more.
pub const fn entry_kind(kind: EntryKind) -> IconKind {
    match kind {
        EntryKind::Folder => IconKind::Folder,
        EntryKind::File => IconKind::File,
    }
}

/// The icon and tint an entry of `kind` is drawn with.
pub const fn kind_glyph(kind: EntryKind, colors: &Colors) -> (Icon, Rgba) {
    built_in(entry_kind(kind), colors)
}

/// The window's own icon for `kind`, and its tint.
pub const fn built_in(kind: IconKind, colors: &Colors) -> (Icon, Rgba) {
    let glyph = match kind {
        IconKind::Folder => return (Icon::Folder, colors.folder),
        IconKind::File => Icon::File,
        IconKind::Text => Icon::FileText,
        IconKind::Picture => Icon::FilePicture,
        IconKind::Video => Icon::FileVideo,
        IconKind::Audio => Icon::FileAudio,
        IconKind::Archive => Icon::FileArchive,
        IconKind::Program => Icon::FileProgram,
    };
    (glyph, colors.text_soft)
}

/// `glyph`, `side` across, in `tint`.
pub fn icon(glyph: Icon, side: Pixels, tint: Rgba) -> impl IntoElement {
    svg()
        .path(glyph.path())
        .flex_none()
        .size(side)
        .text_color(Hsla::from(tint))
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
        // Rows 0, 1 and 2 selected, 3 not, 4 selected alone.
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
        // No row comes after the last one there can be.
        assert!(Corners::of(usize::MAX, |_| true).bottom);
    }
}
