use gpui::{App, Global, Rgba, rgb};
use nova_files_api::{Color, Mode, Palette, Theme};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colors {
    pub window: Rgba,
    pub sheet: Rgba,
    pub raised: Rgba,
    pub lifted: Rgba,
    pub shadow: Rgba,
    pub hover: Rgba,
    pub press: Rgba,
    pub selected: Rgba,
    pub scrim: Rgba,
    pub line: Rgba,
    pub ring: Rgba,
    pub text: Rgba,
    pub text_soft: Rgba,
    pub text_dim: Rgba,
    pub text_faint: Rgba,
    pub folder: Rgba,
    pub here: Rgba,
    pub caret: Rgba,
    pub full: Rgba,
    pub selection: Rgba,
    pub mark: Rgba,
    pub accept: Rgba,
    pub on_accept: Rgba,
    pub danger: Rgba,
    pub glow: Rgba,
    pub drop: Rgba,
    pub drop_edge: Rgba,
    pub changed: Rgba,
    pub added: Rgba,
    pub tags: Tags,
    pub code: Code,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Code {
    pub plain: Rgba,
    pub comment: Rgba,
    pub keyword: Rgba,
    pub quoted: Rgba,
    pub number: Rgba,
    pub function: Rgba,
    pub kind: Rgba,
    pub tag: Rgba,
    pub attribute: Rgba,
    pub line_number: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tags {
    pub image: Rgba,
    pub document: Rgba,
    pub code: Rgba,
    pub archive: Rgba,
    pub media: Rgba,
    pub other: Rgba,
}

mod share {
    pub const SHEET: f32 = 0.20;
    pub const RAISED: f32 = 0.44;
    pub const LIFTED: f32 = 0.72;
    pub const HOVER: f32 = 0.06;
    pub const PRESS: f32 = 0.10;
    pub const SELECTED: f32 = 0.13;
    pub const LINE: f32 = 0.09;
    pub const RING: f32 = 0.46;
    pub const SOFT: f32 = 0.84;
    pub const DIM_DARK: f32 = 0.62;
    pub const DIM_LIGHT: f32 = 0.70;
    pub const FAINT: f32 = 0.38;
    pub const FOLDER: f32 = 0.26;
    pub const SELECTION: f32 = 0.40;
    pub const SHADOW_DARK: f32 = 0.55;
    pub const SHADOW_LIGHT: f32 = 0.22;
    pub const GLOW: f32 = 0.10;
    pub const MARK: f32 = 0.34;
    pub const DROP: f32 = 0.16;
    pub const SCRIM: f32 = 0.72;
    pub const CODE_KIND: f32 = 0.55;
    pub const CODE_COMMENT: f32 = 0.48;
}

impl Colors {
    pub fn new(theme: &Theme) -> Self {
        let palette = &theme.palette;
        let into_bg = |top, share| mix(color(top), share, color(palette.bg));
        let dim_share = match theme.mode {
            Mode::Dark => share::DIM_DARK,
            Mode::Light => share::DIM_LIGHT,
        };
        let shadow = match theme.mode {
            Mode::Dark => thin(SHADOW_DARK_BASE, share::SHADOW_DARK),
            Mode::Light => thin(palette.fg, share::SHADOW_LIGHT),
        };
        let text_dim = into_bg(palette.fg, dim_share);
        Self {
            window: color(palette.bg),
            sheet: into_bg(palette.bg_alt, share::SHEET),
            raised: into_bg(palette.bg_alt, share::RAISED),
            lifted: into_bg(palette.bg_alt, share::LIFTED),
            shadow,
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
            full: color(palette.error),
            selection: thin(palette.secondary, share::SELECTION),
            mark: thin(palette.primary, share::MARK),
            accept: color(palette.primary),
            on_accept: color(palette.on_accent),
            danger: color(palette.error),
            glow: thin(palette.primary, share::GLOW),
            drop: thin(palette.primary, share::DROP),
            drop_edge: color(palette.primary),
            changed: color(palette.warning),
            added: color(palette.success),
            tags: Tags::new(palette, text_dim),
            code: Code {
                plain: into_bg(palette.fg, share::SOFT),
                comment: into_bg(palette.fg, share::CODE_COMMENT),
                keyword: color(palette.secondary),
                quoted: color(palette.success),
                number: color(palette.warning),
                function: color(palette.info),
                kind: mix(color(palette.info), share::CODE_KIND, color(palette.fg)),
                tag: color(palette.error),
                attribute: mix(color(palette.warning), share::CODE_KIND, color(palette.fg)),
                line_number: into_bg(palette.fg, share::FAINT),
            },
        }
    }

    pub fn of(cx: &App) -> Self {
        cx.try_global::<Self>().copied().unwrap_or_default()
    }
}

impl Tags {
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

const SHADOW_DARK_BASE: Color = Color(0x00_00_00);

fn color(color: Color) -> Rgba {
    rgb(color.0)
}

fn mix(top: Rgba, share: f32, under: Rgba) -> Rgba {
    let blend = |top: f32, under: f32| top.mul_add(share, under * (1.0 - share));
    Rgba {
        r: blend(top.r, under.r),
        g: blend(top.g, under.g),
        b: blend(top.b, under.b),
        a: 1.0,
    }
}

fn thin(top: Color, share: f32) -> Rgba {
    Rgba {
        a: share,
        ..color(top)
    }
}

pub mod window {
    use gpui::{Pixels, px};

    pub const WIDTH: Pixels = px(1100.0);
    pub const HEIGHT: Pixels = px(720.0);
    pub const PICK_WIDTH: Pixels = px(900.0);
    pub const PICK_HEIGHT: Pixels = px(600.0);
    pub const NARROW_WIDTH: Pixels = px(720.0);
}

pub mod text {
    use gpui::{FontWeight, Pixels, px};

    pub const FAMILY: &str = "Inter";
    pub const BODY: Pixels = px(13.0);
    pub const BODY_WEIGHT: FontWeight = FontWeight::MEDIUM;
    pub const DETAIL: Pixels = px(12.5);
    pub const DETAIL_WEIGHT: FontWeight = FontWeight::NORMAL;
    pub const LABEL: Pixels = px(12.0);
    pub const LABEL_WEIGHT: FontWeight = FontWeight::SEMIBOLD;
    pub const STRONG_WEIGHT: FontWeight = FontWeight::SEMIBOLD;
    pub const CARD_NAME: Pixels = px(13.5);
    pub const HERO_NAME: Pixels = px(16.0);
    pub const PERCENT: Pixels = px(26.0);
}

pub mod size {
    use gpui::{Pixels, px};

    pub const GAP: Pixels = px(8.0);
    pub const FIELD_LINE: Pixels = px(20.0);
    pub const CARET_WIDTH: Pixels = px(1.5);
    pub const COMPOSING_UNDERLINE: Pixels = px(1.0);
    pub const ROW_HEIGHT: Pixels = px(38.0);
    pub const ROW_PAD: Pixels = px(12.0);
    pub const NAME_GAP: Pixels = px(11.0);
    pub const COLUMN_GAP: Pixels = px(14.0);
    pub const COLUMN_BLEED: Pixels = px(8.0);
    pub const SHEET_PAD: Pixels = px(8.0);
    pub const ROW_ICON: Pixels = px(22.0);
    pub const ICON_CELL_PAD: Pixels = px(8.0);
    pub const ICON_CELL_APART: Pixels = px(2.0);
    pub const ICON_NAME_GAP: Pixels = px(6.0);
    pub const ICON_CELL_UNDER: Pixels = px(104.0);
    pub const ICON_CELL_BESIDE: Pixels = px(240.0);
    pub const ICON_NAME_LINE: Pixels = px(17.0);
    // GPUI lets a line cut short overrun by about the ellipsis.
    // https://github.com/ImperaLuna/zed/tree/47424cdc3f1a49829960e1ecbcdcdd967d034265/crates/gpui
    pub const ICON_NAME_OVERRUN: Pixels = px(6.0);
    pub const STAGE_PAD: Pixels = px(16.0);
    pub const STAGE_CAPTION: Pixels = px(40.0);
    pub const STRIP_PAD: Pixels = px(8.0);
    pub const ICON_SIDES: [Pixels; 6] = [
        px(48.0),
        px(64.0),
        px(96.0),
        px(128.0),
        px(192.0),
        px(256.0),
    ];
    pub const MODIFIED_WIDTH: Pixels = px(150.0);
    pub const KIND_WIDTH: Pixels = px(140.0);
    pub const SIZE_WIDTH: Pixels = px(92.0);
    pub const HEAD_HEIGHT: Pixels = px(38.0);
    pub const HEAD_TOP: Pixels = px(4.0);
    pub const HEAD_BOTTOM: Pixels = px(4.0);
    pub const HEAD_BUTTON_HEIGHT: Pixels = px(24.0);
    pub const HEAD_BUTTON_PAD: Pixels = px(6.0);
    pub const HEAD_ARROW: Pixels = px(11.0);
    pub const HEAD_ARROW_GAP: Pixels = px(4.0);
    pub const SIDEBAR_WIDTH: Pixels = px(228.0);
    pub const SIDEBAR_NARROWEST: Pixels = px(160.0);
    pub const SIDEBAR_WIDEST: Pixels = px(420.0);
    pub const COLUMN_NARROWEST: Pixels = px(60.0);
    pub const COLUMN_WIDEST: Pixels = px(400.0);
    pub const GRIP_REACH: Pixels = px(8.0);
    pub const GRIP_LINE: Pixels = px(2.0);
    pub const PLACE_HEIGHT: Pixels = px(34.0);
    pub const DRIVES_MAX_HEIGHT: Pixels = px(200.0);
    pub const OPEN_MAX_SHARE: f32 = 0.42;
    pub const TAB_CLOSE: Pixels = px(22.0);
    pub const TAB_CLOSE_ICON: Pixels = px(12.0);
    pub const TAB_CLOSE_RIGHT: Pixels = px(6.0);
    pub const PLACE_PAD: Pixels = px(10.0);
    pub const PLACE_ICON: Pixels = px(19.0);
    pub const PLACE_GAP: Pixels = px(1.0);
    pub const PIN_LINE: Pixels = px(4.0);
    pub const PIN_LINE_REACH: Pixels = px(14.0);
    pub const PIN_LINE_OVERHANG: Pixels = px(-7.0);
    pub const PLACE_NARROW_PAD: Pixels = px(12.0);
    pub const PLACE_NARROW_GAP: Pixels = px(8.0);
    pub const TREE_ROW_HEIGHT: Pixels = px(30.0);
    pub const TREE_ROW_GAP: Pixels = px(6.0);
    pub const TREE_PAD_RIGHT: Pixels = px(8.0);
    pub const TREE_TOGGLE: Pixels = px(20.0);
    pub const TREE_TOGGLE_REACH: Pixels = px(26.0);
    pub const TREE_TOGGLE_PULL: Pixels = px(-4.0);
    pub const TREE_TOGGLE_ICON: Pixels = px(12.0);
    pub const TREE_ICON: Pixels = px(17.0);
    pub const TREE_INDENT: Pixels = px(22.0);
    pub const TREE_GUIDE_INSET: Pixels = px(4.0);
    pub const SECTION_TOP: Pixels = px(8.0);
    pub const SECTION_RIGHT: Pixels = px(6.0);
    pub const SECTION_BOTTOM: Pixels = px(4.0);
    pub const CAPSULE_HEIGHT: Pixels = px(40.0);
    pub const CAPSULE_PAD: Pixels = px(3.0);
    pub const CAPSULE_BUTTON_GAP: Pixels = px(1.0);
    pub const ICON_BUTTON: Pixels = px(34.0);
    pub const BUTTON_ICON: Pixels = px(19.0);
    pub const PATH_MIN_WIDTH: Pixels = px(150.0);
    pub const PATH_FILL_MIN: Pixels = px(12.0);
    pub const PATH_FIELD_PAD: Pixels = px(10.0);
    pub const CRUMB_PAD: Pixels = px(9.0);
    pub const CRUMB_GAP: Pixels = px(7.0);
    pub const CRUMB_ICON: Pixels = px(16.0);
    pub const SEPARATOR_WIDTH: Pixels = px(18.0);
    pub const SEPARATOR_HEIGHT: Pixels = px(26.0);
    pub const SEPARATOR_ICON: Pixels = px(11.0);
    pub const SEARCH_WIDTH: Pixels = px(220.0);
    pub const SEARCH_PAD: Pixels = px(12.0);
    pub const SEARCH_GAP: Pixels = px(9.0);
    pub const FOLDER_WIDTH: Pixels = px(200.0);
    pub const PART_WIDTH: Pixels = px(170.0);
    pub const MARK_DOT: Pixels = px(7.0);
    pub const MARK_GAP: Pixels = px(7.0);
    pub const MARK_WIDTH: Pixels = px(16.0);
    pub const FOLDER_LINK_HEIGHT: Pixels = px(26.0);
    pub const FOLDER_LINK_PAD: Pixels = px(8.0);
    pub const CHIP_HEIGHT: Pixels = px(28.0);
    pub const CHIP_PAD: Pixels = px(10.0);
    pub const STATUS_INSET: Pixels = px(12.0);
    pub const STATUS_PAD_X: Pixels = px(8.0);
    pub const STATUS_PAD_Y: Pixels = px(3.0);
    pub const EMPTY_ICON: Pixels = px(44.0);
    pub const EMPTY_GAP: Pixels = px(10.0);
    pub const CONTROL_HEIGHT: Pixels = px(38.0);
    pub const CONTROL_PAD: Pixels = px(16.0);
    pub const QUESTION_GAP: Pixels = px(8.0);
    pub const RING: Pixels = px(1.5);
    pub const LINE: Pixels = px(1.0);
    pub const DROP_EDGE: Pixels = RING;
    pub const DRAG_CHIP_PAD: Pixels = px(12.0);
    pub const DRAG_CHIP_PAD_Y: Pixels = px(6.0);
    pub const FULL_NAME_WIDTH: Pixels = px(360.0);
    pub const MENU_ROW_HEIGHT: Pixels = px(32.0);
    pub const MENU_PAD: Pixels = px(5.0);
    pub const ISLAND_HEIGHT: Pixels = px(46.0);
    pub const ISLAND_PAD_LEFT: Pixels = px(16.0);
    pub const ISLAND_PAD_RIGHT: Pixels = px(6.0);
    pub const ISLAND_GAP: Pixels = px(4.0);
    pub const ISLAND_TEXT_GAP: Pixels = px(10.0);
    pub const ISLAND_BOTTOM: Pixels = px(10.0);
    pub const ISLAND_MARGIN: Pixels = px(8.0);
    pub const ISLAND_SHADOW_Y: Pixels = px(14.0);
    pub const ISLAND_SHADOW_BLUR: Pixels = px(34.0);
    pub const ISLAND_DIVIDER_HEIGHT: Pixels = px(20.0);
    pub const ISLAND_CLEARANCE: Pixels = px(68.0);
    pub const ISLAND_LIST_WIDTH: Pixels = px(450.0);
    pub const ISLAND_LIST_PAD: Pixels = px(6.0);
    pub const ISLAND_LIST_ROW: Pixels = px(40.0);
    pub const ISLAND_ROW_NAME: Pixels = px(84.0);
    pub const ISLAND_LIST_ROWS: f32 = 6.5;
    pub const ISLAND_LIST_LINE: Pixels = px(34.0);
    pub const ISLAND_LIST_INSET: Pixels = px(10.0);
    pub const ISLAND_LIST_GAP: Pixels = px(10.0);
    pub const ISLAND_LIST_LEFT: Pixels = px(30.0);
    pub const ISLAND_LIST_BAR: Pixels = px(72.0);
    pub const PROGRESS_WIDTH: Pixels = px(140.0);
    pub const PROGRESS_HEIGHT: Pixels = px(6.0);
    pub const PROGRESS_RIGHT: Pixels = px(12.0);
    pub const PASSWORD_WIDTH: Pixels = px(180.0);
    pub const RENAME_HEIGHT: Pixels = px(26.0);
    pub const RENAME_PAD: Pixels = px(7.0);
    pub const TEXT_BUTTON_PAD: Pixels = px(12.0);
    pub const TICK_BOX: Pixels = px(16.0);
    pub const TICK_LINE: Pixels = px(1.5);
    pub const TICK_ICON: Pixels = px(11.0);
    pub const TICK_GAP: Pixels = px(8.0);
    pub const COUNT_LEFT: Pixels = px(6.0);
    pub const COUNT_BOTTOM: Pixels = px(12.0);
    pub const COUNT_PAD_X: Pixels = px(8.0);
    pub const COUNT_PAD_Y: Pixels = px(3.0);
    pub const FILTER_CLEAR: Pixels = px(20.0);
    pub const FILTER_CLEAR_ICON: Pixels = px(14.0);
    pub const MENU_ICON: Pixels = px(16.0);
    pub const SECTION_BUTTON: Pixels = px(24.0);
    pub const SECTION_BUTTON_ICON: Pixels = px(15.0);
    pub const MENU_ITEM_PAD: Pixels = px(10.0);
    pub const MENU_ICON_GAP: Pixels = px(10.0);
    pub const MENU_MIN_WIDTH: Pixels = px(210.0);
    pub const MENU_MAX_WIDTH: Pixels = px(300.0);
    pub const MENU_MAX_HEIGHT: Pixels = px(340.0);
    pub const MENU_OFFSET: Pixels = px(6.0);
    pub const MENU_ITEM_GAP: Pixels = px(1.0);
    pub const MENU_KEY_GAP: Pixels = px(18.0);
    pub const MENU_LINE_Y: Pixels = px(4.0);
    pub const MENU_LINE_X: Pixels = px(8.0);
    pub const MENU_EDGE: Pixels = px(8.0);
    pub const MENU_SHADOW_Y: Pixels = px(18.0);
    pub const MENU_SHADOW_BLUR: Pixels = px(44.0);
    pub const FILTER_MAX_WIDTH: Pixels = px(200.0);
    pub const FILTER_MENU_MAX_HEIGHT: Pixels = px(320.0);
}

pub mod start {
    use gpui::{Pixels, px};

    pub const MAX_WIDTH: Pixels = px(800.0);
    pub const PAD_TOP: Pixels = px(28.0);
    pub const PAD_SIDE: Pixels = px(12.0);
    pub const GROUP_GAP: Pixels = px(30.0);
    pub const LABEL_GAP: Pixels = px(10.0);
    pub const LABEL_INSET: Pixels = px(4.0);
    pub const CARD_PAD: Pixels = px(14.0);
    pub const CARD_GAP: Pixels = px(14.0);
    pub const HERO_PAD: Pixels = px(18.0);
    pub const HERO_GAP: Pixels = px(16.0);
    pub const HERO_HEAD_GAP: Pixels = px(14.0);
    pub const CARD_ICON: Pixels = px(34.0);
    pub const HERO_ICON: Pixels = px(46.0);
    pub const SUBTITLE_GAP: Pixels = px(2.0);
    pub const PEEK_ROW: Pixels = px(28.0);
    pub const PEEK_TOP: Pixels = px(12.0);
    pub const PEEK_GAP: Pixels = px(10.0);
    pub const PEEK_MORE_INSET: Pixels = px(32.0);
    pub const FAN_ICON: Pixels = px(24.0);
    pub const FAN_OVERLAP: Pixels = px(5.0);
    pub const FAN_TILT: f32 = 8.0;
    pub const GLOW_END: f32 = 0.46;
    pub const GLOW_ANGLE: f32 = 180.0;
    pub const DRIVE_ICON: Pixels = px(22.0);
    pub const DRIVE_HEAD_GAP: Pixels = px(10.0);
    pub const BAR: Pixels = px(6.0);
}

pub mod properties {
    use gpui::{Pixels, px};

    pub const CHOICE_GAP: Pixels = px(12.0);
    pub const CHOICE_ARROW: Pixels = px(12.0);
    pub const ACCESS_PAD: Pixels = px(6.0);
    pub const ACCESS_ROW_GAP: Pixels = px(10.0);
    pub const ACCESS_ROW_END: Pixels = px(3.0);
    pub const CHIP_GAP: Pixels = px(4.0);
    pub const CHIP_ARROW: Pixels = px(11.0);
}

pub mod opacity {
    pub const DIMMED: f32 = 0.4;
    pub const HIDDEN: f32 = 0.55;
}

pub mod preview {
    use gpui::{Pixels, px};

    pub const MARGIN: Pixels = px(16.0);
    pub const GAP: Pixels = px(12.0);
    pub const NAV: Pixels = px(40.0);
    pub const CAPTION_HEIGHT: Pixels = px(46.0);
    pub const CAPTION_PAD_START: Pixels = px(16.0);
    pub const CAPTION_PAD_END: Pixels = px(6.0);
    pub const CAPTION_GAP: Pixels = px(4.0);
    pub const CAPTION_TEXT_GAP: Pixels = px(10.0);
    pub const BUTTON_HEIGHT: Pixels = px(34.0);
    pub const BUTTON_PAD: Pixels = px(12.0);
    pub const TEXT_WIDTH: Pixels = px(560.0);
    pub const TEXT_PAD_X: Pixels = px(26.0);
    pub const TEXT_PAD_Y: Pixels = px(22.0);
    pub const TEXT_LINE: Pixels = px(22.0);
    pub const LIST_WIDTH: Pixels = px(400.0);
    pub const LIST_PAD_X: Pixels = px(18.0);
    pub const LIST_PAD_Y: Pixels = px(14.0);
    pub const LIST_ROW: Pixels = px(32.0);
    pub const LIST_GAP: Pixels = px(10.0);
    pub const CARD_WIDTH: Pixels = px(640.0);
    pub const CARD_HEIGHT: Pixels = px(400.0);
    pub const BIG_ICON: Pixels = px(96.0);
}

pub mod space {
    use gpui::{Pixels, px};

    pub const COLUMN_WIDTH: Pixels = px(190.0);
    pub const BAR_WIDTH: Pixels = px(96.0);
    pub const BAR_HEIGHT: Pixels = px(4.0);
    pub const BAR_GAP: Pixels = px(12.0);
    pub const MIN_FILL: f32 = 0.02;
}

pub mod pile {
    use gpui::{Pixels, px};

    pub const MOST: usize = 3;
    pub const CARD: Pixels = px(58.0);
    pub const STEP_X: Pixels = px(9.0);
    pub const STEP_Y: Pixels = px(7.0);
    pub const TILT_DEGREES: f32 = 5.0;
    pub const SEE_THROUGH: f32 = 0.88;
    pub const EDGE: Pixels = px(1.5);
    pub const ICON_PAD: Pixels = px(7.0);
    pub const MARGIN: Pixels = px(10.0);
    pub const SHADOW_Y: Pixels = px(3.0);
    pub const SHADOW_BLUR: Pixels = px(5.0);
    pub const CHIP_HEIGHT: Pixels = px(26.0);
    pub const CHIP_OVERHANG: Pixels = px(10.0);
    pub const NAME_GAP: Pixels = px(6.0);
    pub const NAME_WIDTH: Pixels = px(180.0);
    pub const FROM_POINTER: Pixels = px(12.0);
}

pub mod radius {
    use gpui::{Pixels, px};

    pub const SHEET: Pixels = px(16.0);
    pub const CARD: Pixels = px(16.0);
    pub const CONTROL: Pixels = px(12.0);
    pub const ROW: Pixels = px(10.0);
    pub const SMALL: Pixels = px(6.0);
    pub const TICK: Pixels = px(4.0);
    pub const SHARE_BAR: Pixels = px(2.0);
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
        assert_eq!(colors.danger, color(palette.error));
        assert_eq!(colors.changed, color(palette.warning));
        assert_eq!(colors.added, color(palette.success));
        assert_eq!(colors.shadow, thin(palette.fg, share::SHADOW_LIGHT));
        let dark = Colors::default();
        assert_eq!(dark.shadow, thin(SHADOW_DARK_BASE, share::SHADOW_DARK));
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
