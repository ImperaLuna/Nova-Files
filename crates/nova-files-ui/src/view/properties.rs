//! The Properties page, drawn on the sheet in place of the rows, built
//! from the Start page's parts: its column, cards, labels and spacing.
//!
//! The top card has the drive card's layout: the icon, the name with one
//! dim line under it (what it is and where), and the size large on the
//! right. Several items show a fan of their kinds, how many they are and
//! where. Under it, for one item, three small cards say when it was
//! modified, made and last opened, each a date over its name; a date the
//! drive doesn't keep has no card. Until the core answers, the page is
//! empty.
//!
//! Debug selectors: `properties` for the page, `properties-top` for the
//! top card and `properties-date-<n>` for each date card shown.

use std::ffi::OsStr;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _,
};
use nova_files_api::{EntryKind, IconKind, PathKind, Peeked, Properties};

use super::row::{built_in, glyph_of, icon};
use super::start::{fan, name_of, name_os, titles, where_is};
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
                    }),
            )
    }

    /// The first group: the top card, and the dates of a lone item.
    fn about(&self, read: &[Arc<Properties>], colors: &Colors) -> AnyElement {
        let (name, line) = self.titles_of(read);
        let top = match read {
            [one] => top_card(
                icon(lone_glyph(one), start::HERO_ICON, lone_tint(one, colors)).into_any_element(),
                name,
                line,
                lone_size(one),
                colors,
            ),
            many => top_card(
                fan(&peeked(many), colors).into_any_element(),
                name,
                line,
                several_size(many),
                colors,
            ),
        };
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

    /// The top card's name and the dim line under it, once the page on
    /// screen has its answers.
    pub fn properties_titles(&self) -> Option<(String, String)> {
        let read = self.properties_page()?.read();
        (!read.is_empty()).then(|| self.titles_of(&read))
    }

    /// The name and line of `read`: a lone item's name, with what it is
    /// and where; or how many items there are, of what, and where.
    fn titles_of(&self, read: &[Arc<Properties>]) -> (String, String) {
        match read {
            [one] => (name_of(&one.path), lone_line(one, &self.home)),
            many => (
                island::count(many.len(), ITEM, ITEMS),
                self.several_line(many),
            ),
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

    /// The icon of the lone path a Properties page shows, once read: its
    /// kind's, or `None` for a folder, whose own icon stays.
    pub fn properties_icon(&self) -> Option<Icon> {
        match &self.properties_page()?.read()[..] {
            [one] if one.kind != PathKind::Folder => Some(lone_glyph(one)),
            _ => None,
        }
    }
}

/// The top card: `head` (an icon or a fan), the name over one dim line,
/// and the size large on the right when there is one.
fn top_card(
    head: AnyElement,
    name: String,
    line: String,
    amount: Option<String>,
    colors: &Colors,
) -> AnyElement {
    div()
        .debug_selector(|| "properties-top".to_owned())
        .flex()
        .items_center()
        .gap(start::HERO_HEAD_GAP)
        .min_w_0()
        .p(start::HERO_PAD)
        .rounded(radius::CARD)
        .bg(colors.raised)
        .child(head)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(titles(name, line, text::HERO_NAME, colors)),
        )
        .when_some(amount, |card, amount| {
            card.child(
                div()
                    .flex_none()
                    .text_size(text::PERCENT)
                    .font_weight(text::STRONG_WEIGHT)
                    .text_color(colors.text)
                    .child(amount),
            )
        })
        .into_any_element()
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

/// A lone item's size: a file's or a link's own; a folder's comes from
/// measuring what it holds.
fn lone_size(one: &Properties) -> Option<String> {
    (one.kind != PathKind::Folder).then(|| columns::size(one.bytes))
}

/// What several items take, when none is a folder, whose size comes
/// from measuring what it holds.
fn several_size(read: &[Arc<Properties>]) -> Option<String> {
    read.iter()
        .all(|one| one.kind != PathKind::Folder)
        .then(|| columns::size(read.iter().map(|one| one.bytes).sum()))
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
        assert_eq!(lone_size(&png).as_deref(), Some("6.1 MB"));
        let bare = read("/etc/hostname", PathKind::File, 9);
        assert_eq!(lone_line(&bare, home), "File · /etc");
        let folder = read("/home/me/Projects", PathKind::Folder, 4096);
        assert_eq!(lone_line(&folder, home), "~");
        assert_eq!(lone_size(&folder), None);
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
    fn several_files_add_up_but_a_folder_waits_for_measuring() {
        let files = [
            Arc::new(read("/a", PathKind::File, 1_500)),
            Arc::new(read("/b", PathKind::File, 500)),
        ];
        assert_eq!(several_size(&files).as_deref(), Some("2.0 KB"));
        let with_folder = [
            Arc::new(read("/a", PathKind::File, 1_500)),
            Arc::new(read("/c", PathKind::Folder, 4096)),
        ];
        assert_eq!(several_size(&with_folder), None);
        let fanned = peeked(&with_folder);
        assert_eq!(fanned[0].kind, EntryKind::File);
        assert_eq!(fanned[1].kind, EntryKind::Folder);
        assert_eq!(fanned[1].name, "c");
    }
}
