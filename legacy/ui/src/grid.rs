use std::ops::Range;

use gpui::{Pixels, Size, size};
use nova_files_folders_api::IconSize;

use crate::theme::size as sizes;
use crate::theme::size::{
    ICON_CELL_BESIDE as NARROWEST_BESIDE, ICON_CELL_PAD as CELL_PAD,
    ICON_CELL_UNDER as NARROWEST_UNDER, ICON_NAME_GAP as NAME_GAP, ICON_NAME_LINE as NAME_LINE,
};

pub const NAME_LINES: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grid {
    pub per_line: usize,
    pub cell: Size<Pixels>,
    pub icon: Pixels,
    pub beside: bool,
    pub names: bool,
}

impl Grid {
    pub const fn details(width: Pixels) -> Self {
        Self {
            per_line: 1,
            cell: size(width, sizes::ROW_HEIGHT),
            icon: sizes::ROW_ICON,
            beside: true,
            names: true,
        }
    }

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
            names: true,
        }
    }

    pub fn strip(icons: IconSize) -> Self {
        let icon = icon_side(icons);
        Self {
            per_line: 1,
            cell: size(padded(icon), padded(icon)),
            icon,
            beside: false,
            names: false,
        }
    }

    pub fn relaid(&self, drawn: &Self) -> bool {
        self != drawn
    }

    pub const fn lines(&self, len: usize) -> usize {
        len.div_ceil(self.per_line)
    }

    pub const fn line_of(&self, index: usize) -> usize {
        index / self.per_line
    }

    pub fn entries_of(&self, lines: &Range<usize>, len: usize) -> Range<usize> {
        let at = |line: usize| line.saturating_mul(self.per_line).min(len);
        at(lines.start)..at(lines.end)
    }

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

    pub const fn column_of(&self, index: usize) -> usize {
        index % self.per_line
    }
}

fn padded(inside: Pixels) -> Pixels {
    inside + CELL_PAD + CELL_PAD
}

pub const fn icon_side(icons: IconSize) -> Pixels {
    sizes::ICON_SIDES[icons.index()]
}

#[expect(clippy::cast_precision_loss, reason = "a name takes two lines at most")]
const fn name_lines() -> f32 {
    NAME_LINES as f32
}

#[cfg(test)]
mod tests {
    use gpui::px;

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
        let grid = Grid::icons(IconSize::Medium, px(600.0));
        assert_eq!(grid.per_line, 5);
        assert_eq!(grid.cell.width, px(120.0));
        assert_eq!(grid.icon, px(96.0));
        assert!(!grid.beside);
        assert_eq!(
            grid.cell.height,
            px(96.0) + NAME_GAP + NAME_LINE * 2.0 + CELL_PAD * 2.0
        );
        assert_eq!(Grid::icons(IconSize::Small, px(600.0)).per_line, 5);
        let narrow = Grid::icons(IconSize::Largest, px(100.0));
        assert_eq!(narrow.per_line, 1);
        assert_eq!(narrow.cell.width, px(208.0));
    }

    #[test]
    fn a_grid_is_relaid_when_its_lines_hold_another_number() {
        let wide = Grid::icons(IconSize::Medium, px(600.0));
        assert!(!wide.relaid(&Grid::icons(IconSize::Medium, px(600.0))));
        assert!(wide.relaid(&Grid::icons(IconSize::Medium, px(610.0))));
        assert!(wide.relaid(&Grid::icons(IconSize::Medium, px(300.0))));
    }

    #[test]
    fn the_strip_is_icons_alone() {
        let strip = Grid::strip(IconSize::Small);
        assert_eq!(strip.per_line, 1);
        assert_eq!(strip.cell, size(px(80.0), px(80.0)));
        assert_eq!(strip.icon, px(64.0));
        assert!(!strip.names);
        assert!(Grid::details(px(10.0)).names && Grid::icons(IconSize::Small, px(10.0)).names);
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
