//! Design tokens. Every color and size the UI draws with is named here and
//! nowhere else. The colors come from the core's theme (see [`Colors`]);
//! sizes and corners are fixed.

use gpui::{App, Global, Rgba, rgb};
use nova_files_api::{Color, Mode, Palette, Theme};

/// The colors the UI draws with, worked out from the core's theme.
///
/// As on the design page: a quiet window of `fg` mixed over `bg`, the
/// primary color only for what acts and the open place, and the first
/// swatch for folders. One for the whole app, since every window shows the
/// same theme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colors {
    /// The window's base surface, behind the folder's entries; also a
    /// text field's box.
    pub surface: Rgba,
    /// The places column, the toolbar and the pick bar, one step below the
    /// surface.
    pub surface_sunk: Rgba,
    /// Hairlines between the toolbar, the places and the entries.
    pub divider: Rgba,
    /// A row or button under the pointer.
    pub hover: Rgba,
    /// The place that is open.
    pub open: Rgba,
    /// Selected entries, and the band dragged to select.
    pub selected: Rgba,
    /// The ring around the row the keyboard is on, and the band's edge.
    pub cursor: Rgba,
    /// Names.
    pub text: Rgba,
    /// The path, the columns, and buttons that do nothing right now.
    pub text_dim: Rgba,
    /// Folder icons among the entries.
    pub folder: Rgba,
    /// File icons, and the icons of places not open.
    pub icon: Rgba,
    /// The open place's icon.
    pub open_icon: Rgba,
    /// A text field's caret.
    pub caret: Rgba,
    /// Selected text in a text field.
    pub selection: Rgba,
    /// The button that does what the bar is for: Open, Save, Keep both.
    pub accept: Rgba,
    /// Text on [`Self::accept`].
    pub on_accept: Rgba,
}

/// How much of `fg` each shade mixes over `bg`, and of `bg` over
/// `backdrop` for the sunk surface.
mod share {
    pub const SUNK: f32 = 0.82;
    pub const OPEN: f32 = 0.06;
    pub const HOVER: f32 = 0.08;
    pub const DIVIDER: f32 = 0.10;
    pub const SELECTED: f32 = 0.13;
    pub const CURSOR: f32 = 0.45;
    pub const DIM: f32 = 0.62;
}

impl Colors {
    /// The colors for `theme`.
    pub fn new(theme: &Theme) -> Self {
        let palette = &theme.palette;
        let over_bg = |share| mix(palette.fg, share, palette.bg);
        let dim = over_bg(share::DIM);
        Self {
            surface: color(palette.bg),
            surface_sunk: mix(palette.bg, share::SUNK, palette.backdrop),
            divider: over_bg(share::DIVIDER),
            hover: over_bg(share::HOVER),
            open: over_bg(share::OPEN),
            selected: over_bg(share::SELECTED),
            cursor: over_bg(share::CURSOR),
            text: color(palette.fg),
            text_dim: dim,
            folder: color(palette.swatches[0]),
            icon: dim,
            open_icon: color(palette.primary),
            caret: color(palette.fg),
            selection: color(palette.bg_alt),
            accept: color(palette.primary),
            on_accept: color(palette.on_accent),
        }
    }

    /// The colors the app draws with now: the core's theme once a window
    /// has it, Carbonfox's dark until then.
    pub fn of(cx: &App) -> Self {
        cx.try_global::<Self>().copied().unwrap_or_default()
    }
}

impl Default for Colors {
    fn default() -> Self {
        Self::new(&Theme {
            mode: Mode::Dark,
            palette: Palette::carbonfox(Mode::Dark),
        })
    }
}

impl Global for Colors {}

fn color(color: Color) -> Rgba {
    rgb(color.0)
}

/// `share` of `top` over the rest `under`, channel by channel.
fn mix(top: Color, share: f32, under: Color) -> Rgba {
    let (top, under) = (color(top), color(under));
    let blend = |top: f32, under: f32| top.mul_add(share, under * (1.0 - share));
    Rgba {
        r: blend(top.r, under.r),
        g: blend(top.g, under.g),
        b: blend(top.b, under.b),
        a: 1.0,
    }
}

/// The main window.
pub mod window {
    use gpui::{Pixels, px};

    /// Starting size; the user resizes from there.
    pub const WIDTH: Pixels = px(1100.0);
    pub const HEIGHT: Pixels = px(720.0);
    /// A picker's starting size: a dialog over the app that asked, so
    /// smaller than the main window.
    pub const PICK_WIDTH: Pixels = px(900.0);
    pub const PICK_HEIGHT: Pixels = px(600.0);
}

/// Sizes and spacing.
pub mod size {
    use gpui::{Pixels, px};

    /// Body text.
    pub const TEXT: Pixels = px(14.0);
    /// The path in the toolbar.
    pub const TEXT_SMALL: Pixels = px(13.0);
    /// A text field's line.
    pub const FIELD_LINE: Pixels = px(20.0);
    /// A text field's caret.
    pub const CARET_WIDTH: Pixels = px(1.5);
    /// The underline under text an input method is composing.
    pub const COMPOSING_UNDERLINE: Pixels = px(1.0);
    /// One row of the entries list or the places list.
    pub const ROW_HEIGHT: Pixels = px(32.0);
    /// A row's icon.
    pub const ROW_ICON: Pixels = px(18.0);
    /// A row's thumbnail, in the icon's slot.
    pub const THUMBNAIL: Pixels = px(24.0);
    /// The Up button's icon.
    pub const BUTTON_ICON: Pixels = px(16.0);
    /// The Modified, Type and Size columns; Name takes the rest.
    pub const MODIFIED_WIDTH: Pixels = px(140.0);
    pub const TYPE_WIDTH: Pixels = px(130.0);
    pub const SIZE_WIDTH: Pixels = px(80.0);
    /// The column header above the entries.
    pub const HEADER_HEIGHT: Pixels = px(28.0);
    /// The places column.
    pub const PLACES_WIDTH: Pixels = px(200.0);
    /// The toolbar above both columns.
    pub const TOOLBAR_HEIGHT: Pixels = px(48.0);
    /// A button or text field in a bar.
    pub const CONTROL_HEIGHT: Pixels = px(34.0);
    /// The pick bar's question line, under its controls.
    pub const QUESTION_GAP: Pixels = px(8.0);
    /// Space around a column's rows and inside a row.
    pub const PAD: Pixels = px(10.0);
    /// Space between an icon and its label.
    pub const GAP: Pixels = px(10.0);
    /// Space between a button's icon and its label.
    pub const BUTTON_GAP: Pixels = px(4.0);
    /// Space between rows.
    pub const ROW_GAP: Pixels = px(2.0);
    /// Hairline width.
    pub const DIVIDER: Pixels = px(1.0);
    /// The widest the type filter's button gets; a longer name is cut.
    pub const FILTER_MAX_WIDTH: Pixels = px(200.0);
    /// The tallest the type menu gets before it scrolls.
    pub const FILTER_MENU_MAX_HEIGHT: Pixels = px(320.0);
}

/// How much of an element shows through.
pub mod opacity {
    /// A file the picker's type filter leaves out: still there, set back.
    pub const DIMMED: f32 = 0.4;
    /// The fill of the box dragged to select.
    pub const BAND: f32 = 0.6;
}

/// Corner radii.
pub mod radius {
    use gpui::{Pixels, px};

    /// A row in either list.
    pub const ROW: Pixels = px(14.0);
    /// A thumbnail in a row.
    pub const THUMBNAIL: Pixels = px(4.0);
    /// A toolbar button.
    pub const BUTTON: Pixels = px(12.0);
    /// The box dragged to select.
    pub const BAND: Pixels = px(4.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shades_mix_fg_over_bg() {
        let theme = Theme {
            mode: Mode::Dark,
            palette: Palette {
                fg: Color(0xFF_FF_FF),
                bg: Color(0x00_00_00),
                backdrop: Color(0xFF_FF_FF),
                ..Palette::carbonfox(Mode::Dark)
            },
        };
        let colors = Colors::new(&theme);
        let grey = |level: f32| Rgba {
            r: level,
            g: level,
            b: level,
            a: 1.0,
        };
        assert_eq!(colors.surface, grey(0.0));
        assert_eq!(colors.text, grey(1.0));
        assert_eq!(colors.text_dim, grey(share::DIM));
        assert_eq!(colors.selected, grey(share::SELECTED));
        assert_eq!(colors.hover, grey(share::HOVER));
        assert_eq!(colors.cursor, grey(share::CURSOR));
        // The sunk surface mixes bg over the backdrop instead.
        assert!((colors.surface_sunk.r - (1.0 - share::SUNK)).abs() < f32::EPSILON);
    }

    #[test]
    fn roles_take_their_palette_colors() {
        let palette = Palette::carbonfox(Mode::Light);
        let colors = Colors::new(&Theme {
            mode: Mode::Light,
            palette: palette.clone(),
        });
        assert_eq!(colors.accept, color(palette.primary));
        assert_eq!(colors.open_icon, color(palette.primary));
        assert_eq!(colors.on_accept, color(palette.on_accent));
        assert_eq!(colors.folder, color(palette.swatches[0]));
        assert_eq!(colors.selection, color(palette.bg_alt));
        assert_ne!(colors, Colors::default());
    }
}
