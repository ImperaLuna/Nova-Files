//! Design tokens. Every color and size the UI draws with is named here and
//! nowhere else. The theme file and portal colors replace these defaults
//! later.

/// The main window.
pub mod window {
    use gpui::{Pixels, px};

    /// Starting size; the user resizes from there.
    pub const WIDTH: Pixels = px(1100.0);
    pub const HEIGHT: Pixels = px(720.0);
}

/// Colors, as 0xRRGGBB.
pub mod color {
    /// The window's base surface.
    pub const SURFACE: u32 = 0x001a_1918;
}
