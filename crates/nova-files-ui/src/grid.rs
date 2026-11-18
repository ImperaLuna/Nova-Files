//! Where the entries sit: one to a line in Details, several to a line in
//! Icons. The list scrolls by lines, each the same height, so the window
//! builds only the lines on screen whichever view shows.
//!
//! In Icons the cells share out the list's width: as many as fit at their
//! narrowest, each widened to fill the line, so a click anywhere on the
//! line lands in a cell. At the smallest size the name sits beside the
//! icon; from the next size up it sits under it, on up to two lines.

use std::ops::Range;

use gpui::{Pixels, Size, px, size};
use nova_files_api::IconSize;

use crate::theme::size as sizes;

/// The narrowest cell with its name under the icon, so short icons still
/// leave room for a name.
const NARROWEST_UNDER: Pixels = px(104.0);
/// The narrowest cell with its name beside the icon.
const NARROWEST_BESIDE: Pixels = px(240.0);
/// Space around a cell's icon and name.
pub const CELL_PAD: Pixels = px(8.0);
/// Space between the icon and the name under it.
pub const NAME_GAP: Pixels = px(6.0);
/// One line of a name under its icon.
pub const NAME_LINE: Pixels = px(17.0);
/// Room either side of a name under its icon for a line cut short.
pub const NAME_OVERRUN: Pixels = px(6.0);
/// The lines a name under its icon may take before it is cut short.
pub const NAME_LINES: usize = 2;

/// How the entries are laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grid {
    /// Entries to a line; one in Details.
    pub per_line: usize,
    /// Each cell's size; in Details, a row.
    pub cell: Size<Pixels>,
    /// The icon's side.
    pub icon: Pixels,
    /// Whether the name sits beside the icon rather than under it.
    pub beside: bool,
}

impl Grid {
    /// Details: a row each, as wide as the list.
    pub const fn details(width: Pixels) -> Self {
        Self {
            per_line: 1,
            cell: size(width, sizes::ROW_HEIGHT),
            icon: sizes::ROW_ICON,
            beside: true,
        }
    }

    /// Icons of `icons` across a list `width` wide.
    pub fn icons(icons: IconSize, width: Pixels) -> Self {
        let icon = icon_side(icons);
        let beside = icons == IconSize::Smallest;
        let (narrowest, height) = if beside {
            (NARROWEST_BESIDE, padded(icon))
        } else {
            let tall = padded(icon + NAME_GAP + NAME_LINE * name_lines());
            (padded(icon).max(NARROWEST_UNDER), tall)
        };
        let fit = (width / narrowest).floor();
        // A list is a few thousand pixels across at most.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a small whole number, kept to at least 1"
        )]
        let per_line = fit.max(1.0) as usize;
        Self {
            per_line,
            cell: size((width / fit.max(1.0)).max(narrowest), height),
            icon,
            beside,
        }
    }

    /// Whether a list drawn as `drawn` puts other entries on its lines
    /// than this grid would, so it has to be drawn again.
    pub const fn relaid(&self, drawn: &Self) -> bool {
        self.per_line != drawn.per_line
    }

    /// The lines `len` entries take.
    pub const fn lines(&self, len: usize) -> usize {
        len.div_ceil(self.per_line)
    }

    /// The line entry `index` is on.
    pub const fn line_of(&self, index: usize) -> usize {
        index / self.per_line
    }

    /// The entries of `lines`, of `len` in all.
    pub fn entries_of(&self, lines: &Range<usize>, len: usize) -> Range<usize> {
        let at = |line: usize| line.saturating_mul(self.per_line).min(len);
        at(lines.start)..at(lines.end)
    }

    /// The column `x` across the list falls in, kept to the line.
    pub fn column_at(&self, x: Pixels) -> usize {
        let column = (x / self.cell.width).floor().max(0.0);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "kept to at least 0, and to the line just after"
        )]
        let column = column as usize;
        column.min(self.per_line - 1)
    }

    /// The column entry `index` is in.
    pub const fn column_of(&self, index: usize) -> usize {
        index % self.per_line
    }
}

/// `inside` with a cell's padding either side.
fn padded(inside: Pixels) -> Pixels {
    inside + CELL_PAD + CELL_PAD
}

/// The side of an icon of `icons`.
pub const fn icon_side(icons: IconSize) -> Pixels {
    sizes::ICON_SIDES[icons.index()]
}

#[expect(clippy::cast_precision_loss, reason = "a name takes two lines at most")]
const fn name_lines() -> f32 {
    NAME_LINES as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn details_is_one_row_to_a_line() {
        let grid = Grid::details(px(600.0));
        assert_eq!(grid.per_line, 1);
        assert_eq!(grid.cell, size(px(600.0), sizes::ROW_HEIGHT));
        assert_eq!(grid.lines(5), 5);
        assert_eq!(grid.line_of(3), 3);
        assert_eq!(grid.column_at(px(590.0)), 0);
    }

    #[test]
    fn icons_share_the_width_out_among_as_many_as_fit() {
        // 96 px icons fit at 112 px, so 5 to 600 px, 120 px each.
        let grid = Grid::icons(IconSize::Medium, px(600.0));
        assert_eq!(grid.per_line, 5);
        assert_eq!(grid.cell.width, px(120.0));
        assert_eq!(grid.icon, px(96.0));
        assert!(!grid.beside);
        assert_eq!(
            grid.cell.height,
            px(96.0) + NAME_GAP + NAME_LINE * 2.0 + CELL_PAD * 2.0
        );
        // Small icons still leave room for a name.
        assert_eq!(Grid::icons(IconSize::Small, px(600.0)).per_line, 5);
        // A list too narrow for one still has one, as wide as it takes.
        let narrow = Grid::icons(IconSize::Largest, px(100.0));
        assert_eq!(narrow.per_line, 1);
        assert_eq!(narrow.cell.width, px(208.0));
    }

    #[test]
    fn a_grid_is_relaid_when_its_lines_hold_another_number() {
        let wide = Grid::icons(IconSize::Medium, px(600.0));
        assert!(!wide.relaid(&Grid::icons(IconSize::Medium, px(610.0))));
        assert!(wide.relaid(&Grid::icons(IconSize::Medium, px(300.0))));
    }

    #[test]
    fn the_smallest_icons_have_their_names_beside_them() {
        let grid = Grid::icons(IconSize::Smallest, px(500.0));
        assert!(grid.beside);
        assert_eq!(grid.per_line, 2);
        assert_eq!(grid.cell, size(px(250.0), px(64.0)));
    }

    #[test]
    fn lines_columns_and_entries_follow_from_the_entries_to_a_line() {
        let grid = Grid::icons(IconSize::Medium, px(600.0));
        assert_eq!(grid.lines(0), 0);
        assert_eq!(grid.lines(11), 3);
        assert_eq!(grid.line_of(4), 0);
        assert_eq!(grid.line_of(5), 1);
        assert_eq!(grid.column_of(7), 2);
        assert_eq!(grid.entries_of(&(1..3), 11), 5..11);
        assert_eq!(grid.entries_of(&(0..1), 11), 0..5);
        assert_eq!(grid.column_at(px(-4.0)), 0);
        assert_eq!(grid.column_at(px(239.0)), 1);
        assert_eq!(grid.column_at(px(240.0)), 2);
        assert_eq!(grid.column_at(px(900.0)), 4);
    }

    #[test]
    fn each_size_has_its_own_icon_side() {
        let sides: Vec<Pixels> = IconSize::ALL.into_iter().map(icon_side).collect();
        assert_eq!(sides, [px(48.0), px(64.0), px(96.0), px(128.0), px(192.0)]);
    }
}
