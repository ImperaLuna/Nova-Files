//! Drag a box to select: a press on the entries list that isn't on a
//! name, moved a few pixels, draws a box, and the rows it touches are
//! selected. Ctrl adds them to what was selected. A press on empty space
//! below the rows that isn't dragged selects nothing.
//!
//! A press on a name is left for dragging files (later). The box's start is
//! kept in the list's content, so it stays put on its row while the list
//! scrolls; the row under the mouse is the cursor, which `reselect` scrolls
//! into view, so dragging past an edge scrolls, and a scroll while the box
//! is held selects what it covers then. Moves and the release are taken on
//! the window's root, so the box follows the mouse outside the list; a move
//! with the button up (released outside the window) ends it, and so does a
//! new folder.
//!
//! The list's rows are all one height, so a row is found from a position
//! by sharing the list's content height out among its rows, whatever is
//! drawn.

use gpui::{
    Bounds, ClickEvent, Context, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, Window, point, px,
};
use nova_files_api::{EntryId, ListingId};

use crate::selection::Selection;
use crate::window::MainWindow;

/// How far the mouse moves before a press becomes a box.
const START: Pixels = px(4.0);

/// A press on the list, until it is released.
#[derive(Debug)]
pub struct Band {
    /// Where the press was, in the list's content.
    from: Point<Pixels>,
    /// Where the mouse is, in the window.
    to: Point<Pixels>,
    /// The selection when the press came, which Ctrl adds to.
    before: Option<Selection>,
    add: bool,
    /// Whether the mouse has moved far enough to draw a box.
    dragging: bool,
}

impl MainWindow {
    /// The left button went down on the entries list, not on a name
    /// (`on_name` says it was).
    pub(crate) fn band_pressed(&mut self, event: &MouseDownEvent, on_name: bool) {
        self.band = (event.button == MouseButton::Left && !on_name).then(|| Band {
            from: self.in_content(event.position),
            to: event.position,
            before: self.selection.clone(),
            add: event.modifiers.secondary(),
            dragging: false,
        });
    }

    /// The mouse moved anywhere in the window.
    pub(crate) fn band_moved(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if self.band.is_none() {
            return;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            self.band = None;
            cx.notify();
            return;
        }
        let to = self.in_content(event.position);
        let Some(band) = &mut self.band else { return };
        band.to = event.position;
        let moved = (to.x - band.from.x).abs().max((to.y - band.from.y).abs());
        if !band.dragging && moved < START {
            return;
        }
        band.dragging = true;
        self.band_select(cx);
    }

    /// The list scrolled: a box being dragged now covers other rows.
    pub(crate) fn band_scrolled(&mut self, cx: &mut Context<Self>) {
        if self.band_dragged() {
            self.band_select(cx);
        }
    }

    /// The button came up anywhere in the window: a box selects what it
    /// covers there, and a press on empty space that wasn't dragged
    /// selects nothing.
    pub(crate) fn band_released(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        let Some(band) = &mut self.band else { return };
        band.to = event.position;
        let (dragging, add) = (band.dragging, band.add);
        if dragging {
            self.band_select(cx);
        }
        self.band = None;
        cx.notify();
        if !dragging && !add && self.row_at(event.position).is_none() {
            self.clear_selection(cx);
        }
    }

    /// Selects the rows the box covers, from its start in the content to
    /// the mouse where the list is scrolled now.
    fn band_select(&mut self, cx: &mut Context<Self>) {
        let Some(band) = &self.band else { return };
        let (from, add, before) = (band.from, band.add, band.before.clone());
        let to = self.in_content(band.to);
        let Some(listing) = self.listing_id() else {
            return;
        };
        let rows = rows_touched(from.y, to.y, self.rows_height(), self.entries_len());
        self.reselect(
            |_, picks| match rows {
                Some((anchor, cursor)) => Some(Selection::banded(
                    before.as_ref(),
                    listing,
                    (EntryId(anchor), EntryId(cursor)),
                    add,
                    picks,
                )),
                None => before.filter(|before| add && before.listing == listing),
            },
            cx,
        );
    }

    /// A click on the entry `id` of `listing`, unless it ends a box
    /// dragged to that row.
    pub(crate) fn row_clicked(
        &mut self,
        entry: (ListingId, EntryId),
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.band_dragged() {
            self.entry_clicked(entry, event, window, cx);
        }
    }

    /// Whether the press under way has become a box, so a click on a row
    /// it ends on isn't a click.
    pub(crate) fn band_dragged(&self) -> bool {
        self.band.as_ref().is_some_and(|band| band.dragging)
    }

    /// The box to draw, in the list's own space, while dragging.
    pub(crate) fn band_box(&self) -> Option<Bounds<Pixels>> {
        let band = self.band.as_ref().filter(|band| band.dragging)?;
        let list = self.list_bounds();
        let offset = self.list_offset();
        let from = band.from + offset;
        let to = band.to - list.origin;
        Some(Bounds::from_corners(
            point(from.x.min(to.x), from.y.min(to.y)),
            point(from.x.max(to.x), from.y.max(to.y)),
        ))
    }

    /// The row of the listing under `position` in the window, if any.
    fn row_at(&self, position: Point<Pixels>) -> Option<u32> {
        let y = self.in_content(position).y;
        rows_touched(y, y, self.rows_height(), self.entries_len())
            .map(|(row, _)| row)
            .filter(|_| self.list_bounds().contains(&position))
    }

    /// `position` in the window as a point in the list's content.
    fn in_content(&self, position: Point<Pixels>) -> Point<Pixels> {
        position - self.list_bounds().origin - self.list_offset()
    }

    fn list_bounds(&self) -> Bounds<Pixels> {
        self.scroll.0.borrow().base_handle.bounds()
    }

    fn list_offset(&self) -> Point<Pixels> {
        self.scroll.0.borrow().base_handle.offset()
    }

    /// The height of all the list's rows together.
    fn rows_height(&self) -> Pixels {
        self.scroll
            .0
            .borrow()
            .last_item_size
            .map_or(px(0.0), |size| size.contents.height)
    }

    fn entries_len(&self) -> usize {
        self.listing
            .as_ref()
            .map_or(0, |listing| listing.entries.len())
    }
}

/// The rows of `len`, `all` tall together, between the content heights
/// `from` and `to`: the row at `from` and the row at `to`, each kept to
/// the rows there are, or `None` when the span misses every row.
fn rows_touched(from: Pixels, to: Pixels, all: Pixels, len: usize) -> Option<(u32, u32)> {
    let last = f64::from(u32::try_from(len.checked_sub(1)?).ok()?);
    if all <= px(0.0) {
        return None;
    }
    // Far past either end counts as one row past it, which is enough to
    // tell what the span misses.
    let row = |y: Pixels| {
        (f64::from(f32::from(y)) * (last + 1.0) / f64::from(f32::from(all)))
            .floor()
            .clamp(-1.0, last + 1.0)
    };
    let (low, high) = (row(from).min(row(to)), row(from).max(row(to)));
    if high < 0.0 || low > last {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a whole number kept to 0..=last, which came from a u32"
    )]
    let clamp = |y: Pixels| row(y).clamp(0.0, last) as u32;
    Some((clamp(from), clamp(to)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_span_touches_the_rows_it_crosses_kept_to_the_rows_there_are() {
        let all = px(50.0);
        assert_eq!(rows_touched(px(5.0), px(25.0), all, 5), Some((0, 2)));
        assert_eq!(rows_touched(px(25.0), px(5.0), all, 5), Some((2, 0)));
        assert_eq!(rows_touched(px(-5.0), px(15.0), all, 5), Some((0, 1)));
        assert_eq!(rows_touched(px(35.0), px(90.0), all, 5), Some((3, 4)));
        assert_eq!(rows_touched(px(50.0), px(90.0), all, 5), None);
        assert_eq!(rows_touched(px(-20.0), px(-5.0), all, 5), None);
        assert_eq!(rows_touched(px(5.0), px(5.0), px(0.0), 0), None);
        assert_eq!(rows_touched(px(5.0), px(5.0), px(0.0), 5), None);
    }
}
