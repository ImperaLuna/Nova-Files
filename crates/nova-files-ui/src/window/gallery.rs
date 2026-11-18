//! Gallery: one entry large on a stage, over a strip of the folder's
//! entries as small icons.
//!
//! The stage is the quick look, docked: it shows the entry the cursor is
//! on and follows the cursor as the quick look does, asking the core for
//! a picture that fits the stage (see `preview`). Opening Gallery selects
//! the first entry when nothing is. Space lifts the stage's entry over the
//! window as the quick look, and Space, Escape or Close put it back. The
//! arrows step through the entries, and Enter opens the selection as in
//! the other views. Pickers show Details in its place.
//!
//! The strip holds as many cells as fit across, the cursor's kept in the
//! middle where it can be. Only those are drawn, and only their details
//! and thumbnails asked for.

use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

use gpui::{Bounds, Context, Pixels, Size, Window, size};
use nova_files_api::{EntryId, ViewMode};

use super::MainWindow;
use crate::grid::Grid;
use crate::selection::Move;
use crate::theme::size as sizes;

/// The strip's two sides of the cursor.
const SIDES: usize = 2;

/// Where Gallery drew last, which the next frame lays out by.
#[derive(Debug, Default, Clone)]
pub struct GalleryBounds(Rc<Cell<Bounds<Pixels>>>);

impl GalleryBounds {
    /// Gallery drew in `bounds`; whether that differs from last time.
    pub fn drawn(&self, bounds: Bounds<Pixels>) -> bool {
        self.0.replace(bounds) != bounds
    }

    fn get(&self) -> Bounds<Pixels> {
        self.0.get()
    }
}

impl MainWindow {
    /// Whether the entries on screen show as Gallery.
    pub fn showing_gallery(&self) -> bool {
        self.pick.is_none() && self.folder_view().mode == ViewMode::Gallery
    }

    /// The window's scale factor when it last drew.
    pub(crate) const fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// Where Gallery drew last.
    pub(crate) const fn gallery_bounds(&self) -> &GalleryBounds {
        &self.sizing.gallery
    }

    /// The room the stage gives what it shows: Gallery less its strip,
    /// the island's room under it, and the caption under the stage.
    pub(crate) fn stage_room(&self) -> Size<Pixels> {
        let all = self.sizing.gallery.get().size;
        let below = self.grid().cell.height
            + sizes::STRIP_PAD
            + sizes::STRIP_PAD
            + sizes::ISLAND_CLEARANCE
            + sizes::STAGE_CAPTION;
        size(
            (all.width - sizes::STAGE_PAD - sizes::STAGE_PAD).max(Pixels::ZERO),
            (all.height - below - sizes::STAGE_PAD - sizes::STAGE_PAD).max(Pixels::ZERO),
        )
    }

    /// The entries the strip shows, out of `len`: as many as fit across,
    /// the cursor's in the middle where it can be.
    pub(crate) fn strip_entries(&self, len: usize) -> Range<usize> {
        let grid = self.grid();
        strip(
            self.sizing.gallery.get().size.width,
            grid,
            self.cursor().unwrap_or_default(),
            len,
        )
    }

    /// Each frame: while Gallery shows, its stage is on the cursor's
    /// entry (the first one, selected, when nothing is), and the core is
    /// asked about the strip's entries. Leaving Gallery takes the stage
    /// down.
    pub(crate) fn keep_gallery(&mut self, window: &Window, cx: &mut Context<Self>) {
        if !self.showing_gallery() {
            if self.quick_looks.docked {
                self.close_preview(None, cx);
            }
            return;
        }
        let Some(listing) = self.listing.as_ref() else {
            return;
        };
        let (id, len) = (listing.id, listing.entries.len());
        if len == 0 {
            return;
        }
        if self.cursor().is_none() {
            self.move_cursor(Move::First, false, cx);
        }
        if self.quick_looks.open.is_none()
            && let Some(cursor) = self.cursor().and_then(|at| u32::try_from(at).ok())
        {
            self.preview_entry(id, EntryId(cursor), None, cx);
            self.quick_looks.docked = true;
        }
        self.entries_drawn(self.strip_entries(len), window, cx);
    }
}

/// The `len` entries a strip `width` wide shows, cells laid out as
/// `grid`, with `cursor` in the middle unless the strip is at an end.
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
    use nova_files_api::IconSize;

    use super::*;

    #[test]
    fn the_strip_keeps_the_cursor_in_the_middle_until_an_end() {
        let grid = Grid::strip(IconSize::Small);
        let width = grid.cell.width * 5.0;
        assert_eq!(strip(width, grid, 0, 20), 0..5);
        assert_eq!(strip(width, grid, 2, 20), 0..5);
        assert_eq!(strip(width, grid, 9, 20), 7..12);
        assert_eq!(strip(width, grid, 19, 20), 15..20);
        // Fewer entries than fit.
        assert_eq!(strip(width, grid, 1, 3), 0..3);
        // Too narrow for one still shows one.
        assert_eq!(strip(px(1.0), grid, 4, 20), 4..5);
    }
}
