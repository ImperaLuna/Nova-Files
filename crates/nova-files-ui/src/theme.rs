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
/// (`primary`) marks only the folder you're in, what acts and a search's
/// matches; purple
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
    /// What floats over the sheet: menus and the island.
    pub lifted: Rgba,
    /// The shadow under what floats.
    pub shadow: Rgba,
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
    /// What warns: a drive's free space when it is nearly full.
    pub full: Rgba,
    /// Selected text in a text field.
    pub selection: Rgba,
    /// The part of a name a search matched.
    pub mark: Rgba,
    /// The button that does what the bar is for: Open, Save, Keep both.
    pub accept: Rgba,
    /// Text on [`Self::accept`].
    pub on_accept: Rgba,
    /// A step that can't be undone: Delete for good.
    pub danger: Rgba,
    /// The soft orange glow at the top of the Start page.
    pub glow: Rgba,
    /// Where dragged files would drop: its fill, and its edge.
    pub drop: Rgba,
    pub drop_edge: Rgba,
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
    /// How dark the shadow under what floats is: black on a dark window,
    /// `fg` on a light one, where black would read as dirt.
    pub const SHADOW_DARK: f32 = 0.55;
    pub const SHADOW_LIGHT: f32 = 0.22;
    /// `primary` laid over the sheet, at the top of the Start page.
    pub const GLOW: f32 = 0.10;
    /// A search's match, under the name's text.
    pub const MARK: f32 = 0.34;
    /// `primary` laid over where dragged files would drop.
    pub const DROP: f32 = 0.16;
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

/// What the shadow is made of on a dark window.
const SHADOW_DARK_BASE: Color = Color(0x00_00_00);

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
    /// Narrower than this, the sidebar folds into one row above the bar.
    pub const NARROW_WIDTH: Pixels = px(720.0);
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
    /// A card's name on the Start page, and the large card's.
    pub const CARD_NAME: Pixels = px(13.5);
    pub const HERO_NAME: Pixels = px(16.0);
    /// How full a drive is, large on its Start card.
    pub const PERCENT: Pixels = px(26.0);
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
    /// The most the Drives section takes: its head and about four rows.
    pub const DRIVES_MAX_HEIGHT: Pixels = px(200.0);
    /// The most of the sidebar's height the Open list takes, as a share;
    /// more tabs scroll inside it.
    pub const OPEN_MAX_SHARE: f32 = 0.42;
    /// A tab's close button in the Open list, its icon, and the space on
    /// its right.
    pub const TAB_CLOSE: Pixels = px(22.0);
    pub const TAB_CLOSE_ICON: Pixels = px(12.0);
    pub const TAB_CLOSE_RIGHT: Pixels = px(6.0);
    pub const PLACE_PAD: Pixels = px(10.0);
    pub const PLACE_ICON: Pixels = px(19.0);
    pub const PLACE_GAP: Pixels = px(1.0);
    /// A place in the sidebar folded into one row: snug around its name.
    pub const PLACE_NARROW_PAD: Pixels = px(12.0);
    pub const PLACE_NARROW_GAP: Pixels = px(8.0);
    /// A row of the sidebar's folder tree: its height, the space between
    /// its parts and at its ends, its arrow's slot and arrow, its icon, how
    /// far each level is indented, and how far before a level's rows its
    /// hairline runs.
    pub const TREE_ROW_HEIGHT: Pixels = px(30.0);
    pub const TREE_ROW_GAP: Pixels = px(6.0);
    pub const TREE_PAD_LEFT: Pixels = px(2.0);
    pub const TREE_PAD_RIGHT: Pixels = px(8.0);
    pub const TREE_TOGGLE: Pixels = px(20.0);
    pub const TREE_TOGGLE_ICON: Pixels = px(12.0);
    pub const TREE_ICON: Pixels = px(17.0);
    pub const TREE_INDENT: Pixels = px(22.0);
    pub const TREE_GUIDE_INSET: Pixels = px(4.0);
    /// The space around a sidebar section's head.
    pub const SECTION_TOP: Pixels = px(8.0);
    pub const SECTION_RIGHT: Pixels = px(6.0);
    pub const SECTION_BOTTOM: Pixels = px(4.0);
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
    /// The Folder column of a search's files (in the trash, Was in), and
    /// the cell's link.
    pub const FOLDER_WIDTH: Pixels = px(200.0);
    pub const FOLDER_LINK_HEIGHT: Pixels = px(26.0);
    pub const FOLDER_LINK_PAD: Pixels = px(8.0);
    /// A chip at the path bar's end: where a search looks, Empty Trash.
    pub const CHIP_HEIGHT: Pixels = px(28.0);
    pub const CHIP_PAD: Pixels = px(10.0);
    /// The count of files a search found, over the sheet's bottom left.
    pub const STATUS_INSET: Pixels = px(12.0);
    pub const STATUS_PAD_X: Pixels = px(8.0);
    pub const STATUS_PAD_Y: Pixels = px(3.0);
    /// What shows in an empty list: a quiet icon over one word or two.
    pub const EMPTY_ICON: Pixels = px(44.0);
    pub const EMPTY_GAP: Pixels = px(10.0);
    /// A button or text field in the pick bar.
    pub const CONTROL_HEIGHT: Pixels = px(38.0);
    pub const CONTROL_PAD: Pixels = px(16.0);
    /// The pick bar's question line, under its controls.
    pub const QUESTION_GAP: Pixels = px(8.0);
    /// The ring around the row the keyboard is on.
    pub const RING: Pixels = px(1.5);
    /// Hairline width.
    pub const LINE: Pixels = px(1.0);
    /// The orange edge around where dragged files would drop. Every drop
    /// target keeps a clear edge this wide, so the color shifts nothing.
    pub const DROP_EDGE: Pixels = RING;
    /// The chip under the pointer while files are dragged: the space
    /// inside it either side, and above and below its text.
    pub const DRAG_CHIP_PAD: Pixels = px(12.0);
    pub const DRAG_CHIP_PAD_Y: Pixels = px(6.0);
    /// A row in a menu.
    pub const MENU_ROW_HEIGHT: Pixels = px(32.0);
    pub const MENU_PAD: Pixels = px(5.0);
    /// The island: its height, the space inside it either side, between
    /// its parts, after its text, and from the sheet's bottom edge, and the
    /// least room it leaves either side of it on a narrow sheet.
    pub const ISLAND_HEIGHT: Pixels = px(46.0);
    pub const ISLAND_PAD_LEFT: Pixels = px(16.0);
    pub const ISLAND_PAD_RIGHT: Pixels = px(6.0);
    pub const ISLAND_GAP: Pixels = px(4.0);
    pub const ISLAND_TEXT_GAP: Pixels = px(10.0);
    pub const ISLAND_BOTTOM: Pixels = px(10.0);
    pub const ISLAND_MARGIN: Pixels = px(8.0);
    /// The shadow under the island: how far down it falls and its blur.
    pub const ISLAND_SHADOW_Y: Pixels = px(14.0);
    pub const ISLAND_SHADOW_BLUR: Pixels = px(34.0);
    /// The line between the island's actions and Clear selection.
    pub const ISLAND_DIVIDER_HEIGHT: Pixels = px(20.0);
    /// Room under the last row, so the island never covers it.
    pub const ISLAND_CLEARANCE: Pixels = px(68.0);
    /// The island opened into a list of jobs: its width, the space inside
    /// it and under the rows, the height of each row and of the line
    /// under them, how far in their text starts, the space between a
    /// row's parts, and a row's seconds left and bar.
    pub const ISLAND_LIST_WIDTH: Pixels = px(450.0);
    pub const ISLAND_LIST_PAD: Pixels = px(6.0);
    pub const ISLAND_LIST_ROW: Pixels = px(40.0);
    /// How many rows the open list of jobs shows before it scrolls; the
    /// half row says there is more.
    pub const ISLAND_LIST_ROWS: f32 = 6.5;
    pub const ISLAND_LIST_LINE: Pixels = px(34.0);
    pub const ISLAND_LIST_INSET: Pixels = px(10.0);
    pub const ISLAND_LIST_GAP: Pixels = px(10.0);
    pub const ISLAND_LIST_LEFT: Pixels = px(30.0);
    pub const ISLAND_LIST_BAR: Pixels = px(72.0);
    /// The bar of a running job on the island, and the space after it.
    pub const PROGRESS_WIDTH: Pixels = px(140.0);
    pub const PROGRESS_HEIGHT: Pixels = px(6.0);
    pub const PROGRESS_RIGHT: Pixels = px(12.0);
    /// The rename field over an entry's name, and the space either side
    /// of its text, which lines up with the name it replaces.
    pub const RENAME_HEIGHT: Pixels = px(26.0);
    pub const RENAME_PAD: Pixels = px(7.0);
    /// A worded button on the island (Undo).
    pub const TEXT_BUTTON_PAD: Pixels = px(12.0);
    /// The "For all" tick on the island: its box, the box's edge, the
    /// check inside, and the space before its words.
    pub const TICK_BOX: Pixels = px(16.0);
    pub const TICK_LINE: Pixels = px(1.5);
    pub const TICK_ICON: Pixels = px(11.0);
    pub const TICK_GAP: Pixels = px(8.0);
    /// The quiet count while the island is down: where it sits from the
    /// sheet's corner, and the space in its backing.
    pub const COUNT_LEFT: Pixels = px(6.0);
    pub const COUNT_BOTTOM: Pixels = px(12.0);
    pub const COUNT_PAD_X: Pixels = px(8.0);
    pub const COUNT_PAD_Y: Pixels = px(3.0);
    pub const MENU_ICON: Pixels = px(16.0);
    /// The button on a sidebar section's head, and its icon.
    pub const SECTION_BUTTON: Pixels = px(24.0);
    pub const SECTION_BUTTON_ICON: Pixels = px(15.0);
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
    /// A right-click menu: the space between its items, before an item's
    /// key, around the line between its groups (above and below, then
    /// either end), the least room it leaves to the window's edges, and
    /// its shadow's fall and blur.
    pub const MENU_ITEM_GAP: Pixels = px(1.0);
    pub const MENU_KEY_GAP: Pixels = px(18.0);
    pub const MENU_LINE_Y: Pixels = px(4.0);
    pub const MENU_LINE_X: Pixels = px(8.0);
    pub const MENU_EDGE: Pixels = px(8.0);
    pub const MENU_SHADOW_Y: Pixels = px(18.0);
    pub const MENU_SHADOW_BLUR: Pixels = px(44.0);
    /// The widest the type filter's button gets; a longer name is cut.
    pub const FILTER_MAX_WIDTH: Pixels = px(200.0);
    /// The tallest the type menu gets before it scrolls.
    pub const FILTER_MENU_MAX_HEIGHT: Pixels = px(320.0);
}

/// The Start page: the column its cards sit in, the cards, the large
/// card's peek inside the folder and the small cards' fan of three.
pub mod start {
    use gpui::{Pixels, px};

    /// The widest the page's column gets, and the space around it.
    pub const MAX_WIDTH: Pixels = px(800.0);
    pub const PAD_TOP: Pixels = px(28.0);
    pub const PAD_SIDE: Pixels = px(12.0);
    /// Between the groups, and between a group's label and its cards.
    pub const GROUP_GAP: Pixels = px(30.0);
    pub const LABEL_GAP: Pixels = px(10.0);
    /// The label's inset, so it lines up with a card's content.
    pub const LABEL_INSET: Pixels = px(4.0);
    /// Inside a small card, and between its icon row and its name.
    pub const CARD_PAD: Pixels = px(14.0);
    pub const CARD_GAP: Pixels = px(14.0);
    /// Inside the large card, and between its head and its peek.
    pub const HERO_PAD: Pixels = px(18.0);
    pub const HERO_GAP: Pixels = px(16.0);
    /// Between the large card's icon and its name.
    pub const HERO_HEAD_GAP: Pixels = px(14.0);
    /// The folder icon on a small card and on the large one.
    pub const CARD_ICON: Pixels = px(34.0);
    pub const HERO_ICON: Pixels = px(46.0);
    /// Between a name and the line under it.
    pub const SUBTITLE_GAP: Pixels = px(2.0);
    /// One line of the large card's peek, and the space above the lines.
    pub const PEEK_ROW: Pixels = px(28.0);
    pub const PEEK_TOP: Pixels = px(12.0);
    pub const PEEK_GAP: Pixels = px(10.0);
    /// The peek's "N more" line starts under the names: a row icon (22)
    /// and the gap after it.
    pub const PEEK_MORE_INSET: Pixels = px(32.0);
    /// One icon of a small card's fan, how far each overlaps the one
    /// before, and how far the outer two lean, in degrees.
    pub const FAN_ICON: Pixels = px(24.0);
    pub const FAN_OVERLAP: Pixels = px(5.0);
    pub const FAN_TILT: f32 = 8.0;
    /// Where the glow at the top of the page has faded out, as a share of
    /// the page's height, and the direction it fades in, in degrees (180
    /// is downward).
    pub const GLOW_END: f32 = 0.46;
    pub const GLOW_ANGLE: f32 = 180.0;
    /// A drive card's icon, the space after it, and its bar's height.
    pub const DRIVE_ICON: Pixels = px(22.0);
    pub const DRIVE_HEAD_GAP: Pixels = px(10.0);
    pub const BAR: Pixels = px(6.0);
}

/// How much of an element shows through.
pub mod opacity {
    /// A file the picker's type filter leaves out: still there, set back.
    pub const DIMMED: f32 = 0.4;
    /// A hidden entry's icon, while hidden files show.
    pub const HIDDEN: f32 = 0.55;
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
    /// The box of a tick.
    pub const TICK: Pixels = px(4.0);
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
        assert_eq!(colors.danger, color(palette.error));
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
