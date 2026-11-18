//! Carbonfox, Nova's own theme: the colors for every key a theme file
//! leaves out, and what a window draws with before the core's theme
//! arrives.

#![expect(
    clippy::unreadable_literal,
    reason = "colors are written as 0xRRGGBB, as in a theme file"
)]

use super::{Color, Mode, Palette};

impl Palette {
    /// Carbonfox's palette for `mode`.
    pub const fn carbonfox(mode: Mode) -> Self {
        match mode {
            Mode::Dark => DARK,
            Mode::Light => LIGHT,
        }
    }
}

const DARK: Palette = Palette {
    bg: Color(0x1D1D1D),
    bg_alt: Color(0x393E46),
    backdrop: Color(0x000000),
    fg: Color(0xFFF6EB),
    primary: Color(0xFD7014),
    secondary: Color(0xBE95FF),
    on_accent: Color(0x1D1D1D),
    info: Color(0x00ADB5),
    success: Color(0x25BE6A),
    warning: Color(0xFAEB92),
    error: Color(0xEE5396),
    swatches: [
        Color(0x5988DB),
        Color(0xE8650E),
        Color(0x07A7A5),
        Color(0x9067CC),
        Color(0x029A43),
        Color(0x049BE7),
        Color(0xDD6098),
        Color(0x9C7C02),
    ],
};

const LIGHT: Palette = Palette {
    bg: Color(0xF6F2EA),
    bg_alt: Color(0xDDD6C8),
    backdrop: Color(0xFFFFFF),
    fg: Color(0x26231F),
    primary: Color(0xC4520A),
    secondary: Color(0x7A4FD0),
    on_accent: Color(0xFFFFFF),
    info: Color(0x00838A),
    success: Color(0x167A43),
    warning: Color(0x8A6D00),
    error: Color(0xC8215F),
    swatches: [
        Color(0x507ED1),
        Color(0xCC5809),
        Color(0x008D88),
        Color(0x7349AB),
        Color(0x017833),
        Color(0x0B84C5),
        Color(0xA62C69),
        Color(0x937506),
    ],
};
