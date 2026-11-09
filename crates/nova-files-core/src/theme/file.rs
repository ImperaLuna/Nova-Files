//! The theme file's text: the colors it sets for dark and for light.
//!
//! The file uses Nova's own theme keys (`themes/carbonfox.toml` in Nova),
//! so Nova, matugen or pywal can write one file for both:
//!
//! ```toml
//! name = "Carbonfox"
//! [dark]
//! bg = "#1D1D1D"
//! primary = "#FD7014"
//! # ... any of the keys below
//! [light]
//! bg = "#F6F2EA"
//! ```
//!
//! Every key may be left out; the core then uses Carbonfox's value for it.
//! As with the settings file, a mistake costs only its own key: a color
//! that isn't `#RRGGBB` keeps the default and is reported, an unknown key
//! is reported and left out. `glassTint` is Nova's and is read without
//! complaint, but Nova Files draws a solid window and doesn't use it.

use std::collections::BTreeMap;

use nova_files_api::{Color, Mode, Palette, PaletteKey, SWATCH_COUNT};
use toml_edit::{DocumentMut, Item};

/// The colors one table sets; what it leaves out isn't there.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides {
    pub colors: BTreeMap<PaletteKey, Color>,
    pub swatches: [Option<Color>; SWATCH_COUNT],
}

impl Overrides {
    /// `palette` with these colors in place of its own.
    pub fn over(&self, mut palette: Palette) -> Palette {
        for (&key, &color) in &self.colors {
            palette.set(key, color);
        }
        for (slot, color) in palette.swatches.iter_mut().zip(self.swatches) {
            if let Some(color) = color {
                *slot = color;
            }
        }
        palette
    }
}

/// What a theme file sets.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemeFile {
    pub dark: Overrides,
    pub light: Overrides,
}

impl ThemeFile {
    /// What the file sets for `mode`.
    pub const fn for_mode(&self, mode: Mode) -> &Overrides {
        match mode {
            Mode::Dark => &self.dark,
            Mode::Light => &self.light,
        }
    }
}

const NAME: &str = "name";
const DARK: &str = "dark";
const LIGHT: &str = "light";
/// Nova's glass tint: known, unused.
const GLASS_TINT: &str = "glassTint";
const SWATCH_PREFIX: &str = "swatch";

/// The colors `text` sets, and what in it was left out and why.
pub fn read(text: &str) -> (ThemeFile, Vec<String>) {
    let mut theme = ThemeFile::default();
    let document = match text.parse::<DocumentMut>() {
        Ok(document) => document,
        Err(error) => return (theme, vec![format!("not TOML, using defaults: {error}")]),
    };
    let mut problems = Vec::new();
    for (key, item) in document.iter() {
        match key {
            NAME => {}
            DARK => theme.dark = read_table(DARK, item, &mut problems),
            LIGHT => theme.light = read_table(LIGHT, item, &mut problems),
            unknown => problems.push(format!("unknown key `{unknown}`")),
        }
    }
    (theme, problems)
}

fn read_table(table_name: &str, item: &Item, problems: &mut Vec<String>) -> Overrides {
    let mut overrides = Overrides::default();
    let Some(table) = item.as_table_like() else {
        problems.push(format!("`{table_name}` is a table"));
        return overrides;
    };
    for (key, value) in table.iter() {
        let color = value.as_str().and_then(Color::parse);
        if let Some(palette_key) = PaletteKey::from_name(key) {
            match color {
                Some(color) => {
                    overrides.colors.insert(palette_key, color);
                }
                None => problems.push(bad_color(table_name, key)),
            }
        } else if let Some(slot) =
            swatch_index(key).and_then(|index| overrides.swatches.get_mut(index))
        {
            match color {
                Some(color) => *slot = Some(color),
                None => problems.push(bad_color(table_name, key)),
            }
        } else if key != GLASS_TINT {
            problems.push(format!("unknown color `{table_name}.{key}`"));
        }
    }
    overrides
}

fn bad_color(table_name: &str, key: &str) -> String {
    format!("`{table_name}.{key}` is a \"#RRGGBB\" color; using the default")
}

/// The index of `swatch1` to `swatch8`, from 0.
fn swatch_index(key: &str) -> Option<usize> {
    let digits = key.strip_prefix(SWATCH_PREFIX)?;
    // Just the number as written: `swatch01` and `swatch+1` aren't keys.
    if digits.starts_with('0') || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let number: usize = digits.parse().ok()?;
    (1..=SWATCH_COUNT).contains(&number).then(|| number - 1)
}

#[cfg(test)]
mod tests;
