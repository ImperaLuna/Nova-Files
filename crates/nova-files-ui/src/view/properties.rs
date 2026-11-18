//! The Properties page, drawn on the sheet in place of the rows, built
//! from the Start page's parts: its column, cards, labels and spacing.
//!
//! The top card has the drive card's layout: the icon, the name with one
//! dim line under it (what it is and where), and the size large on the
//! right. Several items show a fan of their kinds, how many they are and
//! where. Under it, for one item, three small cards say when it was
//! modified, made and last opened, each a date over its name; a date the
//! drive doesn't keep has no card. Then a lone file's Opens with (see
//! `opens_with`), and who may do what (see `access`). Until the core answers, the page is empty.
//!
//! Debug selectors: `properties` for the page, `properties-top` for the
//! top card and `properties-date-<n>` for each date card shown.

mod access;
mod opens_with;

use std::ffi::OsStr;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _,
};
use nova_files_api::{EntryKind, IconKind, PathKind, Peeked, Properties, Size};

use super::row::{built_in, glyph_of, icon};
use super::start::{FAN_ENTRIES, bar, fan, name_of, name_os, titles, where_is};
use crate::assets::Icon;
use crate::columns;
use crate::island::{self, ITEM, ITEMS};
use crate::kind::icon_kind;
use crate::theme::{Colors, radius, size, start, text};
use crate::window::{MainWindow, PropertiesPage};

const MODIFIED_LABEL: &str = "Modified";
const CREATED_LABEL: &str = "Created";
const OPENED_LABEL: &str = "Opened";
const LINK_LABEL: &str = "Link";
const SPECIAL_LABEL: &str = "Special file";
/// A file whose name has no extension.
const FILE_LABEL: &str = "File";
/// Between what an item is and where it is.
const SEPARATOR: &str = " · ";
const FILE_WORD: &str = "file";
const FILES_WORD: &str = "files";
const FOLDER_WORD: &str = "folder";
const FOLDERS_WORD: &str = "folders";
/// Digits between two commas in a large count.
const GROUP: usize = 3;

impl MainWindow {
    /// The Properties page `page`, filling the sheet.
    pub(super) fn properties_view(
        &self,
        page: &PropertiesPage,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let colors = Colors::of(cx);
        let read = page.read();
        div()
            .id("properties")
            .debug_selector(|| "properties".to_owned())
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .rounded_tl(radius::SHEET)
            .child(
                div()
                    .mx_auto()
                    .w_full()
                    .max_w(start::MAX_WIDTH)
                    .pt(start::PAD_TOP)
                    .px(start::PAD_SIDE)
                    .flex()
                    .flex_col()
                    .gap(start::GROUP_GAP)
                    .when(!read.is_empty(), |column| {
                        column.child(self.about(&read, &colors))
                    })
                    .children(match &read[..] {
                        [one] => self.opens_with(one, cx),
                        _ => None,
                    })
                    .when(!read.is_empty(), |column| {
                        column.child(self.access(&read, cx))
                    }),
            )
    }

    /// The first group: the top card, and the dates of a lone item.
    fn about(&self, read: &[Arc<Properties>], colors: &Colors) -> AnyElement {
        let head = match read {
            [one] => {
                icon(lone_glyph(one), start::HERO_ICON, lone_tint(one, colors)).into_any_element()
            }
            many => div()
                .flex_none()
                .w(fan_width())
                .child(fan(&peeked(many), colors))
                .into_any_element(),
        };
        let top = top_card(head, self.top_of(read), colors);
        let dates = match read {
            [one] => date_cards(one, colors),
            _ => None,
        };
        div()
            .flex()
            .flex_col()
            .gap(size::GAP)
            .child(top)
            .children(dates)
            .into_any_element()
    }

    /// What the top card of the page on screen says, once it has its
    /// answers.
    pub fn properties_top(&self) -> Option<TopFacts> {
        let read = self.properties_page()?.read();
        (!read.is_empty()).then(|| self.top_of(&read))
    }

    /// The top card's name and the dim line under it, once the page on
    /// screen has its answers.
    pub fn properties_titles(&self) -> Option<(String, String)> {
        self.properties_top().map(|top| (top.name, top.line))
    }

    /// What the top card says of `read`: a lone folder's name, where and
    /// what it holds, its size and its share of the drive; a lone file's
    /// name, what it is and where, and its size; or how many items there
    /// are, of what, where, and their size.
    fn top_of(&self, read: &[Arc<Properties>]) -> TopFacts {
        let measured = self.measured().map(|measure| measure.shown());
        let amount = measured.map(|size| columns::size(size.bytes));
        match read {
            [one] if one.kind == PathKind::Folder => TopFacts {
                name: name_of(&one.path),
                line: folder_line(&where_is(&one.path, &self.home), measured),
                amount,
                has_bar: true,
                share: measured.and_then(|size| self.share_of_drive(&one.path, size)),
            },
            [one] => TopFacts {
                name: name_of(&one.path),
                line: lone_line(one, &self.home),
                amount: Some(columns::size(one.bytes)),
                has_bar: false,
                share: None,
            },
            many => TopFacts {
                name: island::count(many.len(), ITEM, ITEMS),
                line: self.several_line(many),
                amount,
                has_bar: false,
                share: None,
            },
        }
    }

    /// What several items are and where: "2 folders, 3 files · ~/Downloads".
    fn several_line(&self, read: &[Arc<Properties>]) -> String {
        let folders = read
            .iter()
            .filter(|one| one.kind == PathKind::Folder)
            .count();
        let count = island::quiet_count(folders, read.len() - folders);
        match self.properties_folder() {
            Some(folder) => format!(
                "{count}{SEPARATOR}{}",
                island::short_path(folder, &self.home)
            ),
            None => count,
        }
    }

    /// How much of its drive `size`, measured under `path`, takes, from 0
    /// to 1, once the drive's size is known.
    fn share_of_drive(&self, path: &Path, size: Size) -> Option<f32> {
        let space = self.drive_space_of(path)?;
        share(size.on_disk, space.total)
    }

    /// The icon of the lone path a Properties page shows, once read: its
    /// kind's, or `None` for a folder, whose own icon stays.
    pub fn properties_icon(&self) -> Option<Icon> {
        match &self.properties_page()?.read()[..] {
            [one] if one.kind != PathKind::Folder => Some(lone_glyph(one)),
            _ => None,
        }
    }
}

/// What the top card of a Properties page says.
#[derive(Debug, Clone, PartialEq)]
pub struct TopFacts {
    pub name: String,
    /// The dim line under the name.
    pub line: String,
    /// The size, large on the right, once known.
    pub amount: Option<String>,
    /// Whether it has a bar: a folder's, for its share of the drive.
    pub has_bar: bool,
    /// The bar's share, from 0 to 1, once known.
    pub share: Option<f32>,
}

/// The top card, in the drive card's layout: `head` (an icon or a fan)
/// and what `top` says.
fn top_card(head: AnyElement, top: TopFacts, colors: &Colors) -> AnyElement {
    let row = div()
        .flex()
        .items_center()
        .gap(start::HERO_HEAD_GAP)
        .min_w_0()
        .child(head)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(titles(top.name, top.line, text::HERO_NAME, colors)),
        )
        .when_some(top.amount, |head, amount| {
            head.child(
                div()
                    .flex_none()
                    .text_size(text::PERCENT)
                    .font_weight(text::STRONG_WEIGHT)
                    .text_color(colors.text)
                    .child(amount),
            )
        });
    div()
        .debug_selector(|| "properties-top".to_owned())
        .flex()
        .flex_col()
        .gap(start::HERO_GAP)
        .min_w_0()
        .p(start::HERO_PAD)
        .rounded(radius::CARD)
        .bg(colors.raised)
        .child(row)
        .when(top.has_bar, |card| {
            card.child(bar(top.share, colors.folder, colors))
        })
        .into_any_element()
}

/// A folder's dim line: where it is, then what it holds once measured:
/// "~/Projects · 18,240 files, 1,312 folders". The count of folders
/// leaves out the folder itself.
fn folder_line(place: &str, measured: Option<Size>) -> String {
    let Some(size) = measured else {
        return place.to_owned();
    };
    let held = format!(
        "{}, {}",
        counted(size.files, FILE_WORD, FILES_WORD),
        counted(size.folders.saturating_sub(1), FOLDER_WORD, FOLDERS_WORD),
    );
    if place.is_empty() {
        held
    } else {
        format!("{place}{SEPARATOR}{held}")
    }
}

/// "1 file", "18,240 files".
fn counted(number: u64, one: &str, many: &str) -> String {
    let word = if number == 1 { one } else { many };
    format!("{} {word}", grouped(number))
}

/// `number` with a comma between each three digits: "18,240".
fn grouped(number: u64) -> String {
    let digits = number.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(GROUP) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// How much of `total` `part` is, from 0 to 1; `None` for an empty total.
fn share(part: u64, total: u64) -> Option<f32> {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a bar's share needs no more than f32 holds"
    )]
    let ratio = (total > 0).then(|| (part as f32 / total as f32).min(1.0));
    ratio
}

/// The date cards of `one`, side by side: Modified, Created, Opened,
/// each only when known. `None` when none is.
fn date_cards(one: &Properties, colors: &Colors) -> Option<AnyElement> {
    let cards: Vec<AnyElement> = dates(one)
        .into_iter()
        .enumerate()
        .map(|(index, (date, label))| {
            div()
                .debug_selector(move || format!("properties-date-{index}"))
                .flex_1()
                .min_w_0()
                .p(start::CARD_PAD)
                .rounded(radius::CARD)
                .bg(colors.raised)
                .child(titles(date, label.to_owned(), text::CARD_NAME, colors))
                .into_any_element()
        })
        .collect();
    (!cards.is_empty()).then(|| {
        div()
            .flex()
            .gap(size::GAP)
            .children(cards)
            .into_any_element()
    })
}

/// `one`'s dates as shown, with their names, leaving out those not known.
fn dates(one: &Properties) -> Vec<(String, &'static str)> {
    [
        (one.modified, MODIFIED_LABEL),
        (one.created, CREATED_LABEL),
        (one.accessed, OPENED_LABEL),
    ]
    .into_iter()
    .filter_map(|(time, label)| shown_date(time).map(|date| (date, label)))
    .collect()
}

/// A date as a card shows it; `None` when not known or not writable.
fn shown_date(time: Option<SystemTime>) -> Option<String> {
    time.map(columns::modified).filter(|date| !date.is_empty())
}

/// The kind of icon `one` gets: a folder's, or a file's by its name. A
/// link or a device is a file of its own.
fn lone_kind(one: &Properties) -> IconKind {
    match one.kind {
        PathKind::Folder => IconKind::Folder,
        PathKind::File => icon_kind(name_os(&one.path), EntryKind::File),
        PathKind::Link { .. } | PathKind::Special => IconKind::File,
    }
}

fn lone_glyph(one: &Properties) -> Icon {
    glyph_of(lone_kind(one))
}

fn lone_tint(one: &Properties, colors: &Colors) -> gpui::Rgba {
    built_in(lone_kind(one), colors).1
}

/// The dim line under a lone item's name: what it is, then where. A
/// folder needs no word for what it is; its icon says so.
fn lone_line(one: &Properties, home: &Path) -> String {
    let place = where_is(&one.path, home);
    let what = match one.kind {
        PathKind::Folder => return place,
        PathKind::File => file_kind(name_os(&one.path)),
        PathKind::Link { .. } => LINK_LABEL.to_owned(),
        PathKind::Special => SPECIAL_LABEL.to_owned(),
    };
    if place.is_empty() {
        what
    } else {
        format!("{what}{SEPARATOR}{place}")
    }
}

/// What a file is, by its name: "PNG image", "TXT document", "File".
fn file_kind(name: &OsStr) -> String {
    let word = match icon_kind(name, EntryKind::File) {
        IconKind::Picture => "image",
        IconKind::Text => "document",
        IconKind::Video => "video",
        IconKind::Audio => "audio",
        IconKind::Archive => "archive",
        IconKind::Folder | IconKind::File | IconKind::Program => "file",
    };
    Path::new(name).extension().map_or_else(
        || FILE_LABEL.to_owned(),
        |extension| format!("{} {word}", extension.to_string_lossy().to_uppercase()),
    )
}

/// How wide a full fan is: its icons, each after the first tucked under
/// the one before.
fn fan_width() -> gpui::Pixels {
    #[expect(clippy::cast_precision_loss, reason = "a fan holds three icons")]
    let entries = FAN_ENTRIES as f32;
    start::FAN_ICON * entries - start::FAN_OVERLAP * (entries - 1.0)
}

/// Several items as a fan shows them: a folder or a file each.
fn peeked(read: &[Arc<Properties>]) -> Vec<Peeked> {
    read.iter()
        .map(|one| Peeked {
            name: name_os(&one.path).to_owned(),
            kind: if one.kind == PathKind::Folder {
                EntryKind::Folder
            } else {
                EntryKind::File
            },
            amount: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, UNIX_EPOCH};

    use nova_files_api::Account;

    use super::*;

    #[test]
    fn a_full_fan_is_its_icons_less_each_tuck() {
        let icon = start::FAN_ICON;
        let shown = icon + (icon - start::FAN_OVERLAP) + (icon - start::FAN_OVERLAP);
        assert_eq!(FAN_ENTRIES, 3);
        assert!((fan_width() - shown).abs() < gpui::px(f32::EPSILON));
    }

    #[test]
    fn a_lone_item_is_tinted_as_its_rows_are() {
        let colors = Colors::default();
        let folder = read("/a", PathKind::Folder, 0);
        let notes = read("/a/notes.txt", PathKind::File, 0);
        assert_eq!(lone_tint(&folder, &colors), colors.folder);
        assert_eq!(lone_tint(&notes, &colors), colors.text_soft);
    }

    fn read(path: &str, kind: PathKind, bytes: u64) -> Properties {
        let nobody = Account { id: 0, name: None };
        Properties {
            path: PathBuf::from(path),
            kind,
            bytes,
            on_disk: bytes,
            modified: None,
            accessed: None,
            changed: None,
            created: None,
            mode: 0o644,
            owner: nobody.clone(),
            group: nobody,
            may_change: true,
            groups: Vec::new(),
            read_only: Some(false),
        }
    }

    #[test]
    fn a_file_says_what_it_is_by_its_name_then_where_it_is() {
        let home = Path::new("/home/me");
        let png = read("/home/me/Downloads/wall.png", PathKind::File, 6_100_000);
        assert_eq!(lone_line(&png, home), "PNG image · ~/Downloads");
        let bare = read("/etc/hostname", PathKind::File, 9);
        assert_eq!(lone_line(&bare, home), "File · /etc");
        let folder = read("/home/me/Projects", PathKind::Folder, 4096);
        assert_eq!(lone_line(&folder, home), "~");
        let link = PathKind::Link {
            target: PathBuf::from("x"),
        };
        assert_eq!(lone_line(&read("/tmp/l", link, 1), home), "Link · /tmp");
        assert_eq!(
            lone_line(&read("/dev/null", PathKind::Special, 0), home),
            "Special file · /dev"
        );
        assert_eq!(lone_line(&read("/", PathKind::Folder, 0), home), "");
    }

    #[test]
    fn each_kind_of_file_has_its_word() {
        for (name, kind) in [
            ("a.JPG", "JPG image"),
            ("a.pdf", "PDF document"),
            ("a.txt", "TXT file"),
            ("a.mkv", "MKV video"),
            ("a.flac", "FLAC audio"),
            ("a.zip", "ZIP archive"),
            ("a.xyz", "XYZ file"),
        ] {
            assert_eq!(file_kind(OsStr::new(name)), kind);
        }
    }

    #[test]
    fn only_known_dates_have_a_card_in_order() {
        let mut one = read("/a", PathKind::File, 0);
        assert_eq!(dates(&one).len(), 0);
        let time = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        one.accessed = Some(time);
        one.modified = Some(time);
        let shown = columns::modified(time);
        assert_eq!(
            dates(&one),
            [(shown.clone(), MODIFIED_LABEL), (shown, OPENED_LABEL)]
        );
        // A date past what can be written has no card.
        one.created = Some(UNIX_EPOCH + Duration::from_secs(1 << 62));
        assert_eq!(dates(&one).len(), 2);
    }

    #[test]
    fn a_fan_shows_each_item_as_a_folder_or_a_file() {
        let read = [
            Arc::new(read("/a", PathKind::File, 1_500)),
            Arc::new(read("/c", PathKind::Folder, 4096)),
        ];
        let fanned = peeked(&read);
        assert_eq!(fanned[0].kind, EntryKind::File);
        assert_eq!(fanned[1].kind, EntryKind::Folder);
        assert_eq!(fanned[1].name, "c");
    }

    #[test]
    fn a_folder_says_what_it_holds_once_measured_leaving_itself_out() {
        let size = |files, folders| Size {
            files,
            folders,
            ..Size::default()
        };
        assert_eq!(folder_line("~/Projects", None), "~/Projects");
        assert_eq!(
            folder_line("~/Projects", Some(size(18_240, 1_313))),
            "~/Projects · 18,240 files, 1,312 folders"
        );
        assert_eq!(folder_line("", Some(size(1, 2))), "1 file, 1 folder");
        assert_eq!(folder_line("~", Some(size(0, 1))), "~ · 0 files, 0 folders");
    }

    #[test]
    fn large_counts_get_a_comma_every_three_digits() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000), "1,000");
        assert_eq!(grouped(1_234_567), "1,234,567");
    }

    #[test]
    fn a_share_of_the_drive_runs_from_nothing_to_all_of_it() {
        assert_eq!(share(0, 100), Some(0.0));
        assert_eq!(share(25, 100), Some(0.25));
        assert_eq!(share(300, 100), Some(1.0));
        assert_eq!(share(1, 0), None);
    }
}
