use gpui::{
    AnyElement, ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
};
use nova_files_api::Packing;
use nova_files_shell::Scene;
use nova_files_shell::island::look::{chevron, divider, floating, label, square, worded};
use nova_files_shell::theme::{Colors, size, text};
use nova_files_shell::widgets::tick_box;

use crate::ArchivesPart;

const CANCEL: &str = "Cancel";
const COMPRESS: &str = "Compress";
const ITEMS: &str = " items";
const METHOD: &str = "Method";
const LEVEL: &str = "Level";
const PASSWORD: &str = "Password";
const LOCK: &str = "Lock with a password";
const FASTEST: u8 = 1;
const NORMAL: u8 = 6;
const SMALLEST: u8 = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Seven,
    Zip,
    TarZstd,
    TarGzip,
    TarXz,
}

impl Format {
    const ALL: [Self; 5] = [
        Self::Seven,
        Self::Zip,
        Self::TarZstd,
        Self::TarGzip,
        Self::TarXz,
    ];

    pub fn offered(seven: bool) -> impl Iterator<Item = Self> {
        Self::ALL
            .into_iter()
            .filter(move |format| seven || *format != Self::Seven)
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Seven => ".7z",
            Self::Zip => ".zip",
            Self::TarZstd => ".tar.zst",
            Self::TarGzip => ".tar.gz",
            Self::TarXz => ".tar.xz",
        }
    }

    const fn selector(self) -> &'static str {
        match self {
            Self::Seven => "compress-7z",
            Self::Zip => "compress-zip",
            Self::TarZstd => "compress-tar-zst",
            Self::TarGzip => "compress-tar-gz",
            Self::TarXz => "compress-tar-xz",
        }
    }

    pub const fn locks(self) -> bool {
        matches!(self, Self::Seven | Self::Zip)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Fast,
    Normal,
    Smallest,
}

impl Level {
    const ALL: [Self; 3] = [Self::Fast, Self::Normal, Self::Smallest];

    pub const fn number(self) -> u8 {
        match self {
            Self::Fast => FASTEST,
            Self::Normal => NORMAL,
            Self::Smallest => SMALLEST,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Fast => "Fast",
            Self::Normal => "Normal",
            Self::Smallest => "Smallest",
        }
    }

    const fn selector(self) -> &'static str {
        match self {
            Self::Fast => "compress-fast",
            Self::Normal => "compress-normal",
            Self::Smallest => "compress-smallest",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    pub format: Option<Format>,
    pub zstd: bool,
    pub level: Level,
    pub locked: bool,
}

impl Default for Choice {
    fn default() -> Self {
        Self {
            format: None,
            zstd: false,
            level: Level::Normal,
            locked: false,
        }
    }
}

impl Choice {
    pub const fn format(self, seven: bool) -> Format {
        match self.format {
            Some(Format::Seven) if !seven => Format::Zip,
            Some(format) => format,
            None if seven => Format::Seven,
            None => Format::Zip,
        }
    }

    pub const fn packing(self, seven: bool) -> Packing {
        match self.format(seven) {
            Format::Seven => Packing::SevenZip,
            Format::Zip if self.zstd => Packing::ZipZstd,
            Format::Zip => Packing::ZipDeflate,
            Format::TarZstd => Packing::TarZstd,
            Format::TarGzip => Packing::TarGzip,
            Format::TarXz => Packing::TarXz,
        }
    }

    pub const fn locked(self, seven: bool) -> bool {
        self.locked && self.format(seven).locks()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Picking {
    pub open: bool,
}

pub const fn picking(part: &ArchivesPart, scene: &Scene, _: &gpui::App) -> bool {
    part.picking.is_some() && !scene.picker
}

pub fn island(
    part: &mut ArchivesPart,
    _: &Scene,
    _: &mut Window,
    cx: &mut Context<ArchivesPart>,
) -> Option<AnyElement> {
    let picking = part.picking?;
    let what = part.what(cx);
    let colors = Colors::of(cx);
    let choice = part.choice;
    let chosen = choice.format(part.seven);
    let mut parts = vec![
        label(&colors)
            .flex_1()
            .child(
                div()
                    .text_color(colors.text)
                    .font_weight(text::STRONG_WEIGHT)
                    .child(what),
            )
            .into_any_element(),
    ];
    parts.extend(Format::offered(part.seven).map(|format| {
        chip(format.selector(), format.label(), format == chosen, &colors)
            .on_click(cx.listener(move |part, _: &ClickEvent, _, cx| {
                part.choice.format = Some(format);
                cx.notify();
            }))
            .into_any_element()
    }));
    parts.push(divider(&colors).into_any_element());
    parts.push(
        square("compress-more", &colors)
            .child(chevron(picking.open, &colors))
            .on_click(cx.listener(|part, _: &ClickEvent, _, cx| {
                if let Some(picking) = part.picking.as_mut() {
                    picking.open = !picking.open;
                }
                cx.notify();
            }))
            .into_any_element(),
    );
    parts.push(
        worded("compress-cancel", CANCEL, &colors)
            .on_click(cx.listener(|part, _: &ClickEvent, _, cx| {
                part.picking = None;
                cx.notify();
            }))
            .into_any_element(),
    );
    parts.push(
        worded("compress-make", COMPRESS, &colors)
            .text_color(colors.accept)
            .on_click(cx.listener(|part, _: &ClickEvent, window, cx| {
                part.compress_picked(window, cx);
            }))
            .into_any_element(),
    );
    let above = picking
        .open
        .then(|| more(choice, chosen, &colors, cx).into_any_element());
    Some(floating(parts, above, &colors).into_any_element())
}

pub fn what(names: &[&std::ffi::OsStr]) -> SharedString {
    match names {
        [one] => one.to_string_lossy().into_owned().into(),
        several => format!("{}{ITEMS}", several.len()).into(),
    }
}

fn more(
    choice: Choice,
    chosen: Format,
    colors: &Colors,
    cx: &Context<ArchivesPart>,
) -> impl IntoElement {
    let mut rows = Vec::new();
    if chosen == Format::Zip {
        let methods = [
            ("compress-deflate", "Deflate", false),
            ("compress-zstd", "Zstandard", true),
        ];
        rows.push(row(
            METHOD,
            methods.map(|(selector, words, zstd)| {
                chip(selector, words, choice.zstd == zstd, colors)
                    .on_click(cx.listener(move |part, _: &ClickEvent, _, cx| {
                        part.choice.zstd = zstd;
                        cx.notify();
                    }))
                    .into_any_element()
            }),
            colors,
        ));
    }
    rows.push(row(
        LEVEL,
        Level::ALL.map(|level| {
            chip(
                level.selector(),
                level.label(),
                choice.level == level,
                colors,
            )
            .on_click(cx.listener(move |part, _: &ClickEvent, _, cx| {
                part.choice.level = level;
                cx.notify();
            }))
            .into_any_element()
        }),
        colors,
    ));
    if chosen.locks() {
        let tick = worded("compress-lock", LOCK, colors)
            .gap(size::TICK_GAP)
            .text_color(colors.text_soft)
            .child(tick_box(choice.locked, colors))
            .on_click(cx.listener(|part, _: &ClickEvent, _, cx| {
                part.choice.locked = !part.choice.locked;
                cx.notify();
            }));
        rows.push(row(PASSWORD, [tick.into_any_element()], colors));
    }
    div()
        .debug_selector(|| "compress-more-rows".to_owned())
        .flex()
        .flex_col()
        .pb(size::ISLAND_LIST_PAD)
        .mb(size::ISLAND_LIST_PAD)
        .border_b(size::LINE)
        .border_color(colors.line)
        .children(rows)
}

fn row(
    name: &'static str,
    chips: impl IntoIterator<Item = AnyElement>,
    colors: &Colors,
) -> AnyElement {
    div()
        .flex()
        .items_center()
        .h(size::ISLAND_LIST_ROW)
        .pl(size::ISLAND_LIST_INSET)
        .gap(size::ISLAND_GAP)
        .child(
            div()
                .flex_none()
                .w(size::ISLAND_ROW_NAME)
                .text_color(colors.text_soft)
                .child(name),
        )
        .children(chips)
        .into_any_element()
}

fn chip(
    selector: &'static str,
    words: &'static str,
    chosen: bool,
    colors: &Colors,
) -> gpui::Stateful<gpui::Div> {
    let chip = worded(selector, words, colors);
    if chosen {
        chip.bg(colors.press).text_color(colors.text)
    } else {
        chip.text_color(colors.text_soft)
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    #[test]
    fn the_first_format_is_7z_when_installed_and_zip_otherwise() {
        let choice = Choice::default();
        assert_eq!(choice.packing(true), Packing::SevenZip);
        assert_eq!(choice.packing(false), Packing::ZipDeflate);
        let seven = Choice {
            format: Some(Format::Seven),
            ..choice
        };
        assert_eq!(seven.packing(false), Packing::ZipDeflate);
        assert_eq!(Format::offered(false).count(), 4);
        assert_eq!(Format::offered(true).next(), Some(Format::Seven));
    }

    #[test]
    fn zip_takes_its_method_and_only_zip_and_7z_lock() {
        let zstd = Choice {
            format: Some(Format::Zip),
            zstd: true,
            locked: true,
            ..Choice::default()
        };
        assert_eq!(zstd.packing(true), Packing::ZipZstd);
        assert!(zstd.locked(true));
        let tar = Choice {
            format: Some(Format::TarXz),
            ..zstd
        };
        assert_eq!(tar.packing(true), Packing::TarXz);
        assert!(!tar.locked(true));
        assert_eq!(
            [Level::Fast, Level::Normal, Level::Smallest].map(Level::number),
            [1, 6, 9]
        );
    }

    #[test]
    fn one_item_is_named_and_several_are_counted() {
        assert_eq!(what(&[OsStr::new("notes.txt")]), "notes.txt");
        assert_eq!(what(&[OsStr::new("a"), OsStr::new("b")]), "2 items");
    }
}
