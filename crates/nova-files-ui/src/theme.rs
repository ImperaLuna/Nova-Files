//! Design tokens. Every color and size the UI draws with is named here and
//! nowhere else. The colors come from the core's theme (see [`Colors`]);
//! sizes, corners and type are fixed (the design page's Shape and Type).

use gpui::{App, Global, Rgba, rgb};
use nova_files_api::{Color, Mode, Palette, Theme};

/// The colors the UI draws with, worked out from the core's theme base
/// colors; the theme file needs no keys of its own for them.
///
/// As on the design page: the sidebar and the top bar sit on the window's
/// `bg`; the file sheet and everything raised are `bg_alt` mixed into
/// `bg`, so height never comes from adding white. Hover, press, selection
/// and hairlines are `fg` laid thinly over whatever is under them. Orange
/// (`primary`) marks only the folder you're in and what acts; purple
/// (`secondary`) only selected text. One for the whole app, since every
/// window shows the same theme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colors {
    /// The window itself, under the sidebar, the top bar and the pick bar.
    pub window: Rgba,
    /// The sheet the files sit on.
    pub sheet: Rgba,
    /// The top bar's capsules and a text field's box.
    pub raised: Rgba,
    /// What floats over the sheet: menus.
    pub lifted: Rgba,
    /// A row or button under the pointer.
    pub hover: Rgba,
    /// A button held down, and the place that is open.
    pub press: Rgba,
    /// Selected rows, and the band dragged to select.
    pub selected: Rgba,
    /// Laid over the window under the quick look.
    pub scrim: Rgba,
    /// Hairlines: under the column heads, around menus.
    pub line: Rgba,
    /// The ring around the row the keyboard is on, and the band's edge.
    pub ring: Rgba,
    /// The current folder and what is selected.
    pub text: Rgba,
    /// Names, places and buttons.
    pub text_soft: Rgba,
    /// Dates, sizes, column heads, place icons and buttons that do nothing
    /// right now.
    pub text_dim: Rgba,
    /// The quietest marks.
    pub text_faint: Rgba,
    /// Folder icons.
    pub folder: Rgba,
    /// The folder you're in: its place's icon.
    pub here: Rgba,
    /// A text field's caret.
    pub caret: Rgba,
    /// Selected text in a text field.
    pub selection: Rgba,
    /// The button that does what the bar is for: Open, Save, Keep both.
    pub accept: Rgba,
    /// Text on [`Self::accept`].
    pub on_accept: Rgba,
    /// The tag on a file icon, by the file's kind (see `kind`).
    pub tags: Tags,
}

/// The colored tag a file icon carries for its kind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tags {
    pub image: Rgba,
    pub document: Rgba,
    pub code: Rgba,
    pub archive: Rgba,
    pub media: Rgba,
    pub other: Rgba,
}

/// How much of one base color each shade mixes into another.
mod share {
    /// `bg_alt` into `bg`.
    pub const SHEET: f32 = 0.20;
    pub const RAISED: f32 = 0.44;
    pub const LIFTED: f32 = 0.72;
    /// `fg` laid over what is under it.
    pub const HOVER: f32 = 0.06;
    pub const PRESS: f32 = 0.10;
    pub const SELECTED: f32 = 0.13;
    pub const LINE: f32 = 0.09;
    pub const RING: f32 = 0.46;
    /// `fg` into `bg` for the dimmer text levels. Dim text needs more
    /// `fg` on a light window to stay readable.
    pub const SOFT: f32 = 0.84;
    pub const DIM_DARK: f32 = 0.62;
    pub const DIM_LIGHT: f32 = 0.70;
    pub const FAINT: f32 = 0.38;
    /// `primary` into dim text, for folders.
    pub const FOLDER: f32 = 0.26;
    /// `secondary` laid over a text field, for its selected text.
    pub const SELECTION: f32 = 0.40;
    pub const SCRIM: f32 = 0.72;
}

impl Colors {
    /// The colors for `theme`.
    pub fn new(theme: &Theme) -> Self {
        let palette = &theme.palette;
        let into_bg = |top, share| mix(color(top), share, color(palette.bg));
        let dim_share = match theme.mode {
            Mode::Dark => share::DIM_DARK,
            Mode::Light => share::DIM_LIGHT,
        };
        let text_dim = into_bg(palette.fg, dim_share);
        Self {
            window: color(palette.bg),
            sheet: into_bg(palette.bg_alt, share::SHEET),
            raised: into_bg(palette.bg_alt, share::RAISED),
            lifted: into_bg(palette.bg_alt, share::LIFTED),
            hover: thin(palette.fg, share::HOVER),
            press: thin(palette.fg, share::PRESS),
            selected: thin(palette.fg, share::SELECTED),
            scrim: thin(palette.bg, share::SCRIM),
            line: thin(palette.fg, share::LINE),
            ring: thin(palette.fg, share::RING),
            text: color(palette.fg),
            text_soft: into_bg(palette.fg, share::SOFT),
            text_dim,
            text_faint: into_bg(palette.fg, share::FAINT),
            folder: mix(color(palette.primary), share::FOLDER, text_dim),
            here: color(palette.primary),
            caret: color(palette.primary),
            selection: thin(palette.secondary, share::SELECTION),
            accept: color(palette.primary),
            on_accept: color(palette.on_accent),
            tags: Tags::new(palette, text_dim),
        }
    }

    /// The colors the app draws with now: the core's theme once a window
    /// has it, Carbonfox's dark until then.
    pub fn of(cx: &App) -> Self {
        cx.try_global::<Self>().copied().unwrap_or_default()
    }
}

impl Tags {
    /// The status colors at small size, as on the design page; a kind
    /// with no color of its own takes `other`.
    fn new(palette: &Palette, other: Rgba) -> Self {
        Self {
            image: color(palette.info),
            document: color(palette.error),
            code: color(palette.secondary),
            archive: color(palette.warning),
            media: color(palette.success),
            other,
        }
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
fn mix(top: Rgba, share: f32, under: Rgba) -> Rgba {
    let blend = |top: f32, under: f32| top.mul_add(share, under * (1.0 - share));
    Rgba {
        r: blend(top.r, under.r),
        g: blend(top.g, under.g),
        b: blend(top.b, under.b),
        a: 1.0,
    }
}

/// `top` at `share` strength, laid over whatever is drawn under it.
fn thin(top: Color, share: f32) -> Rgba {
    Rgba {
        a: share,
        ..color(top)
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

/// Type: Inter throughout, in the four roles of the design page.
pub mod text {
    use gpui::{FontWeight, Pixels, px};

    pub const FAMILY: &str = "Inter";
    /// Names, menus, places, buttons.
    pub const BODY: Pixels = px(13.0);
    pub const BODY_WEIGHT: FontWeight = FontWeight::MEDIUM;
    /// Dates and sizes.
    pub const DETAIL: Pixels = px(12.5);
    pub const DETAIL_WEIGHT: FontWeight = FontWeight::NORMAL;
    /// Column heads and labels.
    pub const LABEL: Pixels = px(12.0);
    pub const LABEL_WEIGHT: FontWeight = FontWeight::SEMIBOLD;
    /// The open place.
    pub const STRONG_WEIGHT: FontWeight = FontWeight::SEMIBOLD;
}

/// Sizes and spacing.
pub mod size {
    use gpui::{Pixels, px};

    /// The one gap: between the sidebar, the bar and the sheet, and from
    /// the window's edge to them.
    pub const GAP: Pixels = px(8.0);
    /// A text field's line.
    pub const FIELD_LINE: Pixels = px(20.0);
    /// A text field's caret.
    pub const CARET_WIDTH: Pixels = px(1.5);
    /// The underline under text an input method is composing.
    pub const COMPOSING_UNDERLINE: Pixels = px(1.0);
    /// One row of the file list.
    pub const ROW_HEIGHT: Pixels = px(38.0);
    /// Space either side of a row's content.
    pub const ROW_PAD: Pixels = px(12.0);
    /// Space between a row's icon and its name.
    pub const NAME_GAP: Pixels = px(11.0);
    /// Space between the columns.
    pub const COLUMN_GAP: Pixels = px(14.0);
    /// Space between the sheet's edge and its rows.
    pub const SHEET_PAD: Pixels = px(8.0);
    /// A file or folder icon in a row, and a thumbnail in its place.
    pub const ROW_ICON: Pixels = px(22.0);
    pub const THUMBNAIL: Pixels = px(22.0);
    /// The colored tag on a file icon: where it sits in the icon's box,
    /// and its size. A plain box, so a row draws one icon, not two.
    pub const TAG_LEFT: Pixels = px(6.2);
    pub const TAG_TOP: Pixels = px(13.4);
    pub const TAG_WIDTH: Pixels = px(6.8);
    pub const TAG_HEIGHT: Pixels = px(2.9);
    /// The Modified and Size columns; Name takes the rest.
    pub const MODIFIED_WIDTH: Pixels = px(150.0);
    pub const SIZE_WIDTH: Pixels = px(92.0);
    /// The column heads above the rows, the space above their text, and
    /// the space between them and the first row.
    pub const HEAD_HEIGHT: Pixels = px(38.0);
    pub const HEAD_TOP: Pixels = px(4.0);
    pub const HEAD_BOTTOM: Pixels = px(4.0);
    /// A column head's button and its sort arrow.
    pub const HEAD_BUTTON_HEIGHT: Pixels = px(24.0);
    pub const HEAD_BUTTON_PAD: Pixels = px(6.0);
    pub const HEAD_ARROW: Pixels = px(11.0);
    pub const HEAD_ARROW_GAP: Pixels = px(4.0);
    /// The sidebar, a place in it, its icon, and the space between places.
    pub const SIDEBAR_WIDTH: Pixels = px(228.0);
    pub const PLACE_HEIGHT: Pixels = px(34.0);
    pub const PLACE_PAD: Pixels = px(10.0);
    pub const PLACE_ICON: Pixels = px(19.0);
    pub const PLACE_GAP: Pixels = px(1.0);
    /// A capsule in the top bar, an icon button in it, and its icon.
    pub const CAPSULE_HEIGHT: Pixels = px(40.0);
    pub const CAPSULE_PAD: Pixels = px(3.0);
    pub const CAPSULE_BUTTON_GAP: Pixels = px(1.0);
    pub const ICON_BUTTON: Pixels = px(34.0);
    pub const BUTTON_ICON: Pixels = px(19.0);
    /// The path bar and the search slot: the path takes what's left.
    pub const PATH_MIN_WIDTH: Pixels = px(150.0);
    /// The path bar's empty part after the crumbs, which always shows.
    pub const PATH_FILL_MIN: Pixels = px(12.0);
    /// Space either side of the path bar's text field.
    pub const PATH_FIELD_PAD: Pixels = px(10.0);
    /// A crumb in the path bar: space either side, between its icon and
    /// its name, and its icon.
    pub const CRUMB_PAD: Pixels = px(9.0);
    pub const CRUMB_GAP: Pixels = px(7.0);
    pub const CRUMB_ICON: Pixels = px(16.0);
    /// The arrow between two crumbs, and its icon.
    pub const SEPARATOR_WIDTH: Pixels = px(18.0);
    pub const SEPARATOR_HEIGHT: Pixels = px(26.0);
    pub const SEPARATOR_ICON: Pixels = px(11.0);
    pub const SEARCH_WIDTH: Pixels = px(220.0);
    pub const SEARCH_PAD: Pixels = px(12.0);
    pub const SEARCH_GAP: Pixels = px(9.0);
    /// A button or text field in the pick bar.
    pub const CONTROL_HEIGHT: Pixels = px(38.0);
    pub const CONTROL_PAD: Pixels = px(16.0);
    /// The pick bar's question line, under its controls.
    pub const QUESTION_GAP: Pixels = px(8.0);
    /// The ring around the row the keyboard is on.
    pub const RING: Pixels = px(1.5);
    /// Hairline width.
    pub const LINE: Pixels = px(1.0);
    /// A row in a menu.
    pub const MENU_ROW_HEIGHT: Pixels = px(32.0);
    pub const MENU_PAD: Pixels = px(5.0);
    /// A menu row with an icon: space either side, and between its icon
    /// and its name.
    pub const MENU_ITEM_PAD: Pixels = px(10.0);
    pub const MENU_ICON_GAP: Pixels = px(10.0);
    /// A menu of folders: its narrowest and widest, its tallest before it
    /// scrolls, and how far below its button it hangs.
    pub const MENU_MIN_WIDTH: Pixels = px(210.0);
    pub const MENU_MAX_WIDTH: Pixels = px(300.0);
    pub const MENU_MAX_HEIGHT: Pixels = px(340.0);
    pub const MENU_OFFSET: Pixels = px(6.0);
    /// The widest the type filter's button gets; a longer name is cut.
    pub const FILTER_MAX_WIDTH: Pixels = px(200.0);
    /// The tallest the type menu gets before it scrolls.
    pub const FILTER_MENU_MAX_HEIGHT: Pixels = px(320.0);
}

/// How much of an element shows through.
pub mod opacity {
    /// A file the picker's type filter leaves out: still there, set back.
    pub const DIMMED: f32 = 0.4;
}

/// The quick look (see `view::preview`).
pub mod preview {
    use gpui::{Pixels, px};

    /// Space between the quick look and the window's edges, and between
    /// its parts.
    pub const MARGIN: Pixels = px(16.0);
    pub const GAP: Pixels = px(12.0);
    /// The round Previous and Next buttons either side.
    pub const NAV: Pixels = px(40.0);
    /// The caption under what is shown, and the space at its ends.
    pub const CAPTION_HEIGHT: Pixels = px(46.0);
    pub const CAPTION_PAD_START: Pixels = px(16.0);
    pub const CAPTION_PAD_END: Pixels = px(6.0);
    pub const CAPTION_GAP: Pixels = px(4.0);
    pub const CAPTION_TEXT_GAP: Pixels = px(10.0);
    /// The caption's Open button.
    pub const BUTTON_HEIGHT: Pixels = px(34.0);
    pub const BUTTON_PAD: Pixels = px(12.0);
    /// A text file's page, its padding and its lines.
    pub const TEXT_WIDTH: Pixels = px(560.0);
    pub const TEXT_PAD_X: Pixels = px(26.0);
    pub const TEXT_PAD_Y: Pixels = px(22.0);
    pub const TEXT_LINE: Pixels = px(22.0);
    /// A folder's list of what it holds, its padding and rows.
    pub const LIST_WIDTH: Pixels = px(400.0);
    pub const LIST_PAD_X: Pixels = px(18.0);
    pub const LIST_PAD_Y: Pixels = px(14.0);
    pub const LIST_ROW: Pixels = px(32.0);
    pub const LIST_GAP: Pixels = px(10.0);
    /// The card a file with nothing to draw shows its icon on, and the
    /// icon.
    pub const CARD_WIDTH: Pixels = px(640.0);
    pub const CARD_HEIGHT: Pixels = px(400.0);
    pub const BIG_ICON: Pixels = px(96.0);
}

/// Corner radii. They nest: the sheet sits one gap inside the window,
/// whose corner is 24, so its own corner is 24 less the gap.
pub mod radius {
    use gpui::{Pixels, px};

    /// The sheet's corner where the sidebar and bar meet.
    pub const SHEET: Pixels = px(16.0);
    /// Menus.
    pub const CARD: Pixels = px(16.0);
    /// A capsule in the top bar, a pick bar button or field.
    pub const CONTROL: Pixels = px(12.0);
    /// A row, a place, an icon button.
    pub const ROW: Pixels = px(10.0);
    /// A thumbnail, a column head's button, the band.
    pub const SMALL: Pixels = px(6.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grey(level: f32, alpha: f32) -> Rgba {
        Rgba {
            r: level,
            g: level,
            b: level,
            a: alpha,
        }
    }

    fn near(left: Rgba, right: Rgba) -> bool {
        [
            (left.r, right.r),
            (left.g, right.g),
            (left.b, right.b),
            (left.a, right.a),
        ]
        .into_iter()
        .all(|(left, right)| (left - right).abs() < 1e-6)
    }

    /// A theme of black `bg`, white `fg` and mid-grey `bg_alt`, so each
    /// shade's share reads straight off its level.
    fn black_and_white(mode: Mode) -> Colors {
        Colors::new(&Theme {
            mode,
            palette: Palette {
                fg: Color(0xFF_FF_FF),
                bg: Color(0x00_00_00),
                bg_alt: Color(0xFF_FF_FF),
                ..Palette::carbonfox(mode)
            },
        })
    }

    #[test]
    fn surfaces_mix_bg_alt_into_bg() {
        let colors = black_and_white(Mode::Dark);
        assert!(near(colors.window, grey(0.0, 1.0)));
        assert!(near(colors.sheet, grey(share::SHEET, 1.0)));
        assert!(near(colors.raised, grey(share::RAISED, 1.0)));
        assert!(near(colors.lifted, grey(share::LIFTED, 1.0)));
    }

    #[test]
    fn hover_press_and_selection_lay_fg_thinly_over_what_is_under() {
        let colors = black_and_white(Mode::Dark);
        assert!(near(colors.hover, grey(1.0, share::HOVER)));
        assert!(near(colors.press, grey(1.0, share::PRESS)));
        assert!(near(colors.selected, grey(1.0, share::SELECTED)));
        assert!(near(colors.scrim, grey(0.0, share::SCRIM)));
        assert!(near(colors.line, grey(1.0, share::LINE)));
        assert!(near(colors.ring, grey(1.0, share::RING)));
    }

    #[test]
    fn text_levels_mix_fg_into_bg_and_dim_text_is_stronger_on_light() {
        let dark = black_and_white(Mode::Dark);
        assert!(near(dark.text, grey(1.0, 1.0)));
        assert!(near(dark.text_soft, grey(share::SOFT, 1.0)));
        assert!(near(dark.text_dim, grey(share::DIM_DARK, 1.0)));
        assert!(near(dark.text_faint, grey(share::FAINT, 1.0)));
        let light = black_and_white(Mode::Light);
        assert!(near(light.text_dim, grey(share::DIM_LIGHT, 1.0)));
    }

    #[test]
    fn roles_take_their_palette_colors() {
        let palette = Palette::carbonfox(Mode::Light);
        let colors = Colors::new(&Theme {
            mode: Mode::Light,
            palette: palette.clone(),
        });
        assert_eq!(colors.here, color(palette.primary));
        assert_eq!(colors.accept, color(palette.primary));
        assert_eq!(colors.caret, color(palette.primary));
        assert_eq!(colors.on_accept, color(palette.on_accent));
        assert_eq!(colors.selection, thin(palette.secondary, share::SELECTION));
        assert_eq!(colors.tags.image, color(palette.info));
        assert_eq!(colors.tags.document, color(palette.error));
        assert_eq!(colors.tags.code, color(palette.secondary));
        assert_eq!(colors.tags.archive, color(palette.warning));
        assert_eq!(colors.tags.media, color(palette.success));
        assert_eq!(colors.tags.other, colors.text_dim);
        assert!(near(
            colors.folder,
            mix(color(palette.primary), share::FOLDER, colors.text_dim)
        ));
        assert_ne!(colors, Colors::default());
    }
}
