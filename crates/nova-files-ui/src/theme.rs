//! Design tokens. Every color and size the UI draws with is named here and
//! nowhere else. The theme file and portal colors replace these defaults
//! later.

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

/// Colors, as 0xRRGGBB.
pub mod color {
    /// The window's base surface, behind the folder's entries.
    pub const SURFACE: u32 = 0x001a_1918;
    /// The places column and the toolbar, one step below the surface.
    pub const SURFACE_SUNK: u32 = 0x0016_1514;
    /// Hairlines between the toolbar, the places and the entries.
    pub const DIVIDER: u32 = 0x002a_2724;
    /// A row under the pointer, and the place that is open.
    pub const ROW_HIGHLIGHT: u32 = 0x002a_2724;
    /// Names.
    pub const TEXT: u32 = 0x00d8_ccb0;
    /// The path, and buttons that do nothing right now.
    pub const TEXT_DIM: u32 = 0x008a_857c;
    /// Folder icons.
    pub const FOLDER: u32 = 0x0000_adb5;
    /// File icons.
    pub const FILE: u32 = 0x00a8_a296;
    /// A text field's caret.
    pub const CARET: u32 = 0x00d8_ccb0;
    /// Selected text in a text field.
    pub const SELECTION: u32 = 0x003d_3a35;
    /// Selected entries.
    pub const ROW_SELECTED: u32 = 0x0019_3a3b;
    /// The ring around the row the keyboard is on.
    pub const ROW_CURSOR: u32 = 0x0035_7a7d;
    /// A text field's box.
    pub const FIELD: u32 = 0x001a_1918;
    /// The button that does what the bar is for: Open, Save, Keep both.
    pub const ACCEPT: u32 = 0x00fd_7014;
    /// Text on [`ACCEPT`].
    pub const ON_ACCEPT: u32 = 0x001d_1d1d;
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
    pub const ROW: Pixels = px(10.0);
    /// A thumbnail in a row.
    pub const THUMBNAIL: Pixels = px(4.0);
    /// A toolbar button.
    pub const BUTTON: Pixels = px(10.0);
    /// The box dragged to select.
    pub const BAND: Pixels = px(4.0);
}
