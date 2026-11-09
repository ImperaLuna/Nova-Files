//! Reading theme files key by key.

#![expect(
    clippy::unreadable_literal,
    reason = "colors are written as 0xRRGGBB, as in a theme file"
)]

use super::*;

const NOVA: &str = r##"
name = "Carbonfox"

[dark]
bg = "#1D1D1D"
primary = "#fd7014"
glassTint = "#00000080"
swatch1 = "#5988DB"
swatch8 = "#9C7C02"

[light]
primary = "#C4520A"
"##;

#[test]
fn a_nova_theme_reads_without_complaint() {
    let (theme, problems) = read(NOVA);
    assert_eq!(problems, Vec::<String>::new());
    assert_eq!(
        theme.dark.colors.get(&PaletteKey::Bg),
        Some(&Color(0x1D1D1D))
    );
    assert_eq!(
        theme.dark.colors.get(&PaletteKey::Primary),
        Some(&Color(0xFD7014))
    );
    assert_eq!(theme.dark.swatches[0], Some(Color(0x5988DB)));
    assert_eq!(theme.dark.swatches[7], Some(Color(0x9C7C02)));
    assert_eq!(theme.dark.swatches[1], None);
    assert_eq!(
        theme.light.colors.get(&PaletteKey::Primary),
        Some(&Color(0xC4520A))
    );
    assert_eq!(theme.light.colors.len(), 1);
}

#[test]
fn a_mistake_costs_only_its_own_key() {
    let (theme, problems) = read(
        r##"
colour = "x"
[dark]
bg = "1D1D1D"
fg = "#FFF"
info = 3
primary = "#ABCDEF"
swatch0 = "#000000"
swatch9 = "#000000"
swatch01 = "#000000"
"swatch+1" = "#000000"
swatch2 = "red"
madeUp = "#000000"
"##,
    );
    assert_eq!(theme.dark.colors.len(), 1);
    assert_eq!(
        theme.dark.colors.get(&PaletteKey::Primary),
        Some(&Color(0xABCDEF))
    );
    assert_eq!(theme.dark.swatches, [None; SWATCH_COUNT]);
    assert_eq!(problems.len(), 10, "{problems:?}");
    // Each names the key it is about.
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("`dark.bg`") && problem.contains("#RRGGBB")),
        "{problems:?}"
    );
}

#[test]
fn a_table_that_isnt_one_and_text_that_isnt_toml_read_as_defaults() {
    let (theme, problems) = read("dark = \"#000000\"\n");
    assert_eq!(theme, ThemeFile::default());
    assert_eq!(problems.len(), 1);
    let (theme, problems) = read("[dark");
    assert_eq!(theme, ThemeFile::default());
    assert_eq!(problems.len(), 1);
}

#[test]
fn overrides_replace_only_what_they_name() {
    let (theme, _) = read(NOVA);
    let base = Palette {
        primary: Color(0x111111),
        bg: Color(0x222222),
        ..crate::theme::defaults::carbonfox(Mode::Light)
    };
    let mixed = theme.dark.over(base.clone());
    assert_eq!(mixed.primary, Color(0xFD7014));
    assert_eq!(mixed.bg, Color(0x1D1D1D));
    assert_eq!(mixed.fg, base.fg);
    assert_eq!(mixed.swatches[0], Color(0x5988DB));
    assert_eq!(mixed.swatches[1], base.swatches[1]);
    assert_eq!(theme.for_mode(Mode::Dark), &theme.dark);
    assert_eq!(theme.for_mode(Mode::Light), &theme.light);
}
