//! The theme's colors, as the core resolves them and every window draws
//! with them.
//!
//! The core picks dark or light (the appearance setting, or the desktop's
//! preference on `System`), takes each color from the theme file, then the
//! desktop's accent for [`PaletteKey::Primary`], then Carbonfox, and sends
//! the result as [`crate::Event::Theme`]: when a client asks, and from
//! then on to that client each time it changes. The desktop's preference
//! and accent reach the core as [`DesktopLook`]. These are the base colors
//! only, under the names Nova's theme file uses; the UI works out the
//! shades it draws with from them.

/// A color, as 0xRRGGBB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color(pub u32);

impl Color {
    /// The color written `#RRGGBB` (either case), or `None` for anything
    /// else.
    pub fn parse(text: &str) -> Option<Self> {
        let hex = text.strip_prefix('#')?;
        if hex.len() != HEX_DIGITS || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        u32::from_str_radix(hex, HEX_RADIX).ok().map(Self)
    }
}

/// `RRGGBB`.
const HEX_DIGITS: usize = 6;
const HEX_RADIX: u32 = 16;

/// How many swatches a palette has: colors for things told apart by color
/// (tags, place icons), used in order.
pub const SWATCH_COUNT: usize = 8;

/// Dark or light.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Dark,
    Light,
}

/// What the desktop asks apps to look like, as the freedesktop settings
/// portal says. Each part is `None` when the desktop doesn't say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DesktopLook {
    /// Dark or light, for the `System` appearance.
    pub mode: Option<Mode>,
    /// The accent, the primary color unless the theme file sets one.
    pub accent: Option<Color>,
}

/// The theme every window draws with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub mode: Mode,
    pub palette: Palette,
}

/// The base colors, named as in Nova's theme file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    /// The window's background.
    pub bg: Color,
    /// Solid fills: empty bars, scrollbar thumbs.
    pub bg_alt: Color,
    /// Behind everything; mixed into `bg` for sunk fields.
    pub backdrop: Color,
    /// Text and icons; dimmer levels are worked out from it.
    pub fg: Color,
    /// The accent: current folder, main button, drop target, progress.
    pub primary: Color,
    /// Selected text in fields, tags.
    pub secondary: Color,
    /// Text on a solid accent fill.
    pub on_accent: Color,
    pub info: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub swatches: [Color; SWATCH_COUNT],
}

/// Names one of a palette's base colors (not its swatches).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PaletteKey {
    Bg,
    BgAlt,
    Backdrop,
    Fg,
    Primary,
    Secondary,
    OnAccent,
    Info,
    Success,
    Warning,
    Error,
}

impl PaletteKey {
    pub const ALL: [Self; 11] = [
        Self::Bg,
        Self::BgAlt,
        Self::Backdrop,
        Self::Fg,
        Self::Primary,
        Self::Secondary,
        Self::OnAccent,
        Self::Info,
        Self::Success,
        Self::Warning,
        Self::Error,
    ];

    /// The key's name in a theme file.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Bg => "bg",
            Self::BgAlt => "bgAlt",
            Self::Backdrop => "backdrop",
            Self::Fg => "fg",
            Self::Primary => "primary",
            Self::Secondary => "secondary",
            Self::OnAccent => "onAccent",
            Self::Info => "info",
            Self::Success => "success",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }

    /// The key named `name` in a theme file.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|key| key.name() == name)
    }
}

impl Palette {
    /// The color `key` names.
    pub const fn get(&self, key: PaletteKey) -> Color {
        match key {
            PaletteKey::Bg => self.bg,
            PaletteKey::BgAlt => self.bg_alt,
            PaletteKey::Backdrop => self.backdrop,
            PaletteKey::Fg => self.fg,
            PaletteKey::Primary => self.primary,
            PaletteKey::Secondary => self.secondary,
            PaletteKey::OnAccent => self.on_accent,
            PaletteKey::Info => self.info,
            PaletteKey::Success => self.success,
            PaletteKey::Warning => self.warning,
            PaletteKey::Error => self.error,
        }
    }

    /// Sets the color `key` names.
    pub const fn set(&mut self, key: PaletteKey, color: Color) {
        let slot = match key {
            PaletteKey::Bg => &mut self.bg,
            PaletteKey::BgAlt => &mut self.bg_alt,
            PaletteKey::Backdrop => &mut self.backdrop,
            PaletteKey::Fg => &mut self.fg,
            PaletteKey::Primary => &mut self.primary,
            PaletteKey::Secondary => &mut self.secondary,
            PaletteKey::OnAccent => &mut self.on_accent,
            PaletteKey::Info => &mut self.info,
            PaletteKey::Success => &mut self.success,
            PaletteKey::Warning => &mut self.warning,
            PaletteKey::Error => &mut self.error,
        };
        *slot = color;
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unreadable_literal,
        reason = "colors are written as 0xRRGGBB, as in a theme file"
    )]

    use super::*;

    #[test]
    fn only_hash_and_six_hex_digits_are_a_color() {
        assert_eq!(Color::parse("#FD7014"), Some(Color(0xFD7014)));
        assert_eq!(Color::parse("#fd7014"), Some(Color(0xFD7014)));
        for bad in ["FD7014", "#FD701", "#FD70145", "#FD701G", "#+D7014", ""] {
            assert_eq!(Color::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn every_key_has_its_own_name_and_slot() {
        let mut palette = Palette {
            bg: Color(0),
            bg_alt: Color(0),
            backdrop: Color(0),
            fg: Color(0),
            primary: Color(0),
            secondary: Color(0),
            on_accent: Color(0),
            info: Color(0),
            success: Color(0),
            warning: Color(0),
            error: Color(0),
            swatches: [Color(0); SWATCH_COUNT],
        };
        for (number, key) in (1..).zip(PaletteKey::ALL) {
            assert_eq!(PaletteKey::from_name(key.name()), Some(key));
            palette.set(key, Color(number));
        }
        for (number, key) in (1..).zip(PaletteKey::ALL) {
            assert_eq!(palette.get(key), Color(number), "{key:?}");
        }
        assert_eq!(PaletteKey::from_name("bgalt"), None);
        assert_eq!(palette.on_accent, Color(7));
    }
}
