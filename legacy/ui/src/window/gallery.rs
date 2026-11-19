use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

use gpui::{App, Bounds, Context, Entity, Pixels, Size, Window, size};
use nova_files_folders_api::ViewMode;
use nova_files_preview_ui::PreviewPart;

use super::MainWindow;
use crate::grid::Grid;
use crate::selection::Move;
use crate::theme::size as sizes;

const SIDES: usize = 2;

#[derive(Debug, Default, Clone)]
pub struct GalleryBounds(Rc<Cell<Bounds<Pixels>>>);

impl GalleryBounds {
    pub fn drawn(&self, bounds: Bounds<Pixels>) -> bool {
        self.0.replace(bounds) != bounds
    }

    fn get(&self) -> Bounds<Pixels> {
        self.0.get()
    }
}

impl MainWindow {
    pub(crate) fn preview(&self) -> Option<Entity<PreviewPart>> {
        self.part::<PreviewPart>()
    }

    pub(crate) fn previewing(&self, cx: &App) -> bool {
        self.preview()
            .is_some_and(|preview| preview.read(cx).previewing())
    }

    pub fn showing_gallery(&self) -> bool {
        !self.picker() && self.folder_view().mode == ViewMode::Gallery
    }

    pub(crate) const fn gallery_bounds(&self) -> &GalleryBounds {
        &self.sizing.gallery
    }

    pub(crate) fn stage_room(&self) -> Size<Pixels> {
        stage_room(self.sizing.gallery.get().size, self.grid().cell.height)
    }

    pub(crate) fn strip_entries(&self, len: usize) -> Range<usize> {
        let grid = self.grid();
        strip(
            self.sizing.gallery.get().size.width,
            grid,
            self.cursor().unwrap_or_default(),
            len,
        )
    }

    pub(crate) fn keep_gallery(&mut self, window: &Window, cx: &mut Context<Self>) {
        let preview = self.preview();
        if !self.showing_gallery() {
            if let Some(preview) = preview {
                preview.update(cx, PreviewPart::undock);
            }
            return;
        }
        let Some(listing) = self.listing.as_ref() else {
            return;
        };
        let len = listing.entries.len();
        if len == 0 {
            return;
        }
        if self.cursor().is_none() {
            self.move_cursor(Move::First, false, cx);
        }
        if let (Some(preview), Some(cursor)) = (preview, self.cursor()) {
            preview.update(cx, |preview, cx| preview.dock(cursor, cx));
        }
        self.entries_drawn(self.strip_entries(len), window, cx);
    }
}

fn stage_room(all: Size<Pixels>, cell: Pixels) -> Size<Pixels> {
    let below =
        cell + sizes::STRIP_PAD + sizes::STRIP_PAD + sizes::ISLAND_CLEARANCE + sizes::STAGE_CAPTION;
    size(
        (all.width - sizes::STAGE_PAD - sizes::STAGE_PAD).max(Pixels::ZERO),
        (all.height - below - sizes::STAGE_PAD - sizes::STAGE_PAD).max(Pixels::ZERO),
    )
}

fn strip(width: Pixels, grid: Grid, cursor: usize, len: usize) -> Range<usize> {
    let fit = (width / grid.cell.width).floor().max(1.0);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a small whole number, kept to at least 1"
    )]
    let fit = (fit as usize).min(len);
    let first = cursor.saturating_sub(fit / SIDES).min(len - fit);
    first..first + fit
}

#[cfg(test)]
mod tests {
    use gpui::px;
    use nova_files_folders_api::IconSize;

    use super::*;

    #[test]
    fn the_stage_has_gallery_less_its_strip_caption_and_padding() {
        assert_eq!(
            stage_room(size(px(1000.0), px(800.0)), px(80.0)),
            size(px(968.0), px(564.0))
        );
        assert_eq!(
            stage_room(size(px(20.0), px(100.0)), px(80.0)),
            size(Pixels::ZERO, Pixels::ZERO)
        );
    }

    #[test]
    fn bounds_say_when_they_change() {
        let bounds = GalleryBounds::default();
        let drawn = Bounds::new(gpui::point(px(1.0), px(2.0)), size(px(3.0), px(4.0)));
        assert!(bounds.drawn(drawn));
        assert!(!bounds.drawn(drawn));
        assert_eq!(bounds.get(), drawn);
    }

    #[test]
    fn the_strip_keeps_the_cursor_in_the_middle_until_an_end() {
        let grid = Grid::strip(IconSize::Small);
        let width = grid.cell.width * 5.0;
        assert_eq!(strip(width, grid, 0, 20), 0..5);
        assert_eq!(strip(width, grid, 2, 20), 0..5);
        assert_eq!(strip(width, grid, 9, 20), 7..12);
        assert_eq!(strip(width, grid, 19, 20), 15..20);
        assert_eq!(strip(width, grid, 1, 3), 0..3);
        assert_eq!(strip(px(1.0), grid, 4, 20), 4..5);
    }
}
