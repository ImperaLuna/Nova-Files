//! The pieces of one row in the sheet, shared by the column heads and the
//! entries so their columns line up: the cells, an entry's icon and name,
//! and the ring around the row the keyboard is on. Icons are drawn here
//! for every part of the window.

use std::ffi::OsStr;
use std::sync::Arc;

use gpui::{
    AnyElement, Context, Hsla, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent,
    ParentElement as _, Pixels, RenderImage, Rgba, SharedString, Styled as _, div, img, svg,
};
use nova_files_api::EntryKind;

use crate::assets::Icon;
use crate::kind::FileKind;
use crate::theme::{Colors, radius, size, text};
use crate::window::MainWindow;

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
/// a folder column when given (in the trash, Was in), and Size sits flush
/// right. The Modified cell carries `modified_selector`. Modified and
/// Size are drawn as details: smaller and dimmer.
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

/// An entry's name. A press on it is left for dragging files, so it
/// marks itself for `band`.
pub fn entry_name(name: SharedString, index: usize, cx: &Context<MainWindow>) -> impl IntoElement {
    div()
        .debug_selector(move || format!("name-{index}"))
        .min_w_0()
        .truncate()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, _| this.pressed_name = true),
        )
        .child(name)
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

/// The icon slot of an entry row: its thumbnail if it has one, otherwise
/// its kind's icon, so nothing shifts when the thumbnail arrives. A file's
/// icon carries a tag in its kind's color.
pub fn entry_icon(
    name: &OsStr,
    kind: EntryKind,
    thumbnail: Option<&Arc<RenderImage>>,
    colors: &Colors,
) -> impl IntoElement {
    let slot = div()
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(size::ROW_ICON);
    match (thumbnail, kind) {
        (Some(image), _) => slot.child(
            img(Arc::clone(image))
                .size(size::THUMBNAIL)
                .rounded(radius::SMALL),
        ),
        (None, EntryKind::Folder) => slot.child(icon(Icon::Folder, size::ROW_ICON, colors.folder)),
        (None, EntryKind::File) => slot
            .child(icon(Icon::File, size::ROW_ICON, colors.text_soft))
            .child(
                div()
                    .absolute()
                    .left(size::TAG_LEFT)
                    .top(size::TAG_TOP)
                    .w(size::TAG_WIDTH)
                    .h(size::TAG_HEIGHT)
                    .rounded_full()
                    .bg(FileKind::of(name).tag(colors.tags)),
            ),
    }
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
