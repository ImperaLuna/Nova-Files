use gpui::{
    App, Bounds, ClickEvent, Context, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, Window, point, px,
};
use nova_files_api::{EntryId, ListingId};

use crate::selection::Selection;
use crate::window::MainWindow;

const START: Pixels = px(4.0);

#[derive(Debug)]
pub struct Band {
    from: Point<Pixels>,
    to: Point<Pixels>,
    before: Option<Selection>,
    add: bool,
    dragging: bool,
}

impl MainWindow {
    pub(crate) fn band_pressed(&mut self, event: &MouseDownEvent, on_name: bool, cx: &App) {
        let boxes = event.button == MouseButton::Left && !on_name && !self.showing_gallery(cx);
        self.band = boxes.then(|| Band {
            from: self.in_content(event.position),
            to: event.position,
            before: self.selection_before_press.clone(),
            add: event.modifiers.control || event.modifiers.shift,
            dragging: false,
        });
    }

    pub(crate) fn band_moved(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if self.band.is_none() {
            return;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            self.band = None;
            self.edge_pointer(None, true, cx);
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
        self.edge_pointer(Some(event.position), true, cx);
    }

    pub(crate) fn band_scrolled(&mut self, cx: &mut Context<Self>) {
        if self.band_dragged() {
            self.band_select(cx);
        }
    }

    pub(crate) fn band_released(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        let Some(band) = &mut self.band else { return };
        band.to = event.position;
        let (dragging, add) = (band.dragging, band.add);
        if dragging {
            self.band_select(cx);
        }
        self.band = None;
        self.edge_pointer(None, true, cx);
        cx.notify();
        if !dragging && !add && self.row_at(event.position, cx).is_none() {
            self.clear_selection(cx);
        }
    }

    fn band_select(&mut self, cx: &mut Context<Self>) {
        let Some(band) = &self.band else { return };
        let (from, add, before) = (band.from, band.add, band.before.clone());
        let to = self.in_content(band.to);
        let Some(listing) = self.listing_id() else {
            return;
        };
        let grid = self.grid(cx);
        let len = self.entries_len();
        let lines = rows_touched(from.y, to.y, self.rows_height(), grid.lines(len));
        let columns = (grid.column_at(from.x), grid.column_at(to.x));
        let (left, right) = (columns.0.min(columns.1), columns.0.max(columns.1));
        let id = |at: usize| EntryId(u32::try_from(at).unwrap_or(u32::MAX));
        let corner = |line: u32, column: usize| {
            id(usize::try_from(line)
                .unwrap_or(usize::MAX)
                .saturating_mul(grid.per_line)
                .saturating_add(column)
                .min(len.saturating_sub(1)))
        };
        self.reselect_in_place(
            |_, picks| match lines {
                Some((anchor, cursor)) => {
                    let (low, high) = (anchor.min(cursor), anchor.max(cursor));
                    let lines = usize::try_from(low).unwrap_or(usize::MAX)
                        ..usize::try_from(high).map_or(usize::MAX, |high| high + 1);
                    let covered = grid
                        .entries_of(&lines, len)
                        .filter(|&at| (left..=right).contains(&grid.column_of(at)))
                        .map(id)
                        .filter(|&id| picks(id));
                    Some(Selection::banded(
                        before.as_ref(),
                        listing,
                        covered.collect::<Vec<_>>(),
                        (corner(anchor, columns.0), corner(cursor, columns.1)),
                        add,
                    ))
                }
                None => before.filter(|before| add && before.listing == listing),
            },
            cx,
        );
    }

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

    pub(crate) fn band_dragged(&self) -> bool {
        self.band.as_ref().is_some_and(|band| band.dragging)
    }

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

    fn row_at(&self, position: Point<Pixels>, cx: &App) -> Option<usize> {
        let at = self.in_content(position);
        let grid = self.grid(cx);
        let len = self.entries_len();
        let line = rows_touched(at.y, at.y, self.rows_height(), grid.lines(len))?.0;
        let entry = usize::try_from(line)
            .ok()?
            .checked_mul(grid.per_line)?
            .checked_add(grid.column_at(at.x))?;
        (entry < len && self.list_bounds().contains(&position)).then_some(entry)
    }

    fn in_content(&self, position: Point<Pixels>) -> Point<Pixels> {
        position - self.list_bounds().origin - self.list_offset()
    }

    pub(crate) fn list_bounds(&self) -> Bounds<Pixels> {
        self.scroll.0.borrow().base_handle.bounds()
    }

    fn list_offset(&self) -> Point<Pixels> {
        self.scroll.0.borrow().base_handle.offset()
    }

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

fn rows_touched(from: Pixels, to: Pixels, all: Pixels, len: usize) -> Option<(u32, u32)> {
    let last = f64::from(u32::try_from(len.checked_sub(1)?).ok()?);
    if all <= px(0.0) {
        return None;
    }
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
