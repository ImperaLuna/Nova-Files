//! The Start page, drawn in the sheet's place when the window is on Start:
//! a soft orange glow at the top, then "Jump back in", the folders a
//! person opened lately that the sidebar doesn't already show.
//!
//! The newest is a large card with the folder's first entries (each
//! file's size or each folder's item count) and how many more it holds;
//! up to four smaller cards beside it show each folder's icon with a fan
//! of three entries and where the folder is. A click on a card opens its
//! folder. With no recent folders the group isn't drawn. Each card
//! carries a debug selector, `recent-<n>`, counted from 0.

use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    Radians, SharedString, StatefulInteractiveElement as _, Styled as _, Transformation, div,
    linear_color_stop, linear_gradient, prelude::FluentBuilder as _, svg,
};
use nova_files_api::{EntryKind, Peeked, RecentFolder};

use super::row::{entry_icon, icon};
use crate::assets::Icon;
use crate::columns;
use crate::theme::{Colors, radius, size, start, text};
use crate::window::MainWindow;

const RECENT_LABEL: &str = "Jump back in";
/// How a path under home starts.
const HOME_MARK: &str = "~";
/// Between the folder and its date under the large card's name.
const SEPARATOR: &str = " · ";
/// The small cards sit two by two beside the large one.
const SMALL_PER_ROW: usize = 2;
/// The entries a small card's fan shows.
const FAN_ENTRIES: usize = 3;

impl MainWindow {
    /// The Start page, filling the sheet.
    pub(super) fn start_page(&self, cx: &Context<Self>) -> impl IntoElement {
        let colors = Colors::of(cx);
        div()
            .id("start")
            .debug_selector(|| "start".to_owned())
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .rounded_tl(radius::SHEET)
            .bg(linear_gradient(
                start::GLOW_ANGLE,
                linear_color_stop(colors.glow, 0.0),
                linear_color_stop(colors.sheet, start::GLOW_END),
            ))
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
                    .when(!self.recent.is_empty(), |page| {
                        page.child(group(RECENT_LABEL, self.recent_cards(cx), &colors))
                    }),
            )
    }

    /// The large card for the newest folder on the left half, and the
    /// small ones beside it, two by two, stretched to its height.
    fn recent_cards(&self, cx: &Context<Self>) -> AnyElement {
        let colors = Colors::of(cx);
        let mut folders = self.recent.iter().enumerate();
        let Some((_, newest)) = folders.next() else {
            return div().into_any_element();
        };
        let mut small = folders
            .map(|(index, folder)| self.small_card(index, folder, cx))
            .peekable();
        let mut rows = Vec::new();
        while small.peek().is_some() {
            rows.push(
                div()
                    .flex()
                    .flex_1()
                    .gap(size::GAP)
                    .children(small.by_ref().take(SMALL_PER_ROW)),
            );
        }
        div()
            .flex()
            .gap(size::GAP)
            .child(
                Self::card(0, &newest.path, cx)
                    .flex_1()
                    .p(start::HERO_PAD)
                    .gap(start::HERO_GAP)
                    .child(hero_head(newest, &self.home, &colors))
                    .child(peek(newest, &colors)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(size::GAP)
                    .children(rows),
            )
            .into_any_element()
    }

    /// A small card: the folder's icon and a fan of its entries, then its
    /// name and where it is.
    fn small_card(&self, index: usize, folder: &RecentFolder, cx: &Context<Self>) -> AnyElement {
        let colors = Colors::of(cx);
        Self::card(index, &folder.path, cx)
            .relative()
            .flex_1()
            .p(start::CARD_PAD)
            .gap(start::CARD_GAP)
            .child(icon(Icon::Folder, start::CARD_ICON, colors.folder))
            .child(
                div()
                    .absolute()
                    .top(start::CARD_PAD)
                    .right(start::CARD_PAD)
                    .child(fan(&folder.peek, &colors)),
            )
            .child(titles(
                name_of(&folder.path),
                where_is(&folder.path, &self.home),
                text::CARD_NAME,
                &colors,
            ))
            .into_any_element()
    }

    /// A card's box, which opens `folder` when clicked.
    fn card(index: usize, folder: &Path, cx: &Context<Self>) -> gpui::Stateful<gpui::Div> {
        let colors = Colors::of(cx);
        let target = folder.to_owned();
        div()
            .id(("recent", index))
            .debug_selector(move || format!("recent-{index}"))
            .flex()
            .flex_col()
            .min_w_0()
            .rounded(radius::CARD)
            .bg(colors.raised)
            .cursor_pointer()
            .hover(|card| card.bg(colors.lifted))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, _| this.open(&target)))
    }
}

/// A labelled group of cards.
fn group(label: &'static str, cards: AnyElement, colors: &Colors) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(start::LABEL_GAP)
        .child(
            div()
                .pl(start::LABEL_INSET)
                .text_size(text::LABEL)
                .font_weight(text::LABEL_WEIGHT)
                .text_color(colors.text_dim)
                .child(label),
        )
        .child(cards)
}

/// The large card's head: a big folder icon, the name, and where the
/// folder is with when it last changed.
fn hero_head(folder: &RecentFolder, home: &Path, colors: &Colors) -> impl IntoElement {
    let mut subtitle = where_is(&folder.path, home);
    if let Some(modified) = folder.modified.map(columns::modified)
        && !modified.is_empty()
    {
        subtitle = format!("{subtitle}{SEPARATOR}{modified}");
    }
    div()
        .flex()
        .items_center()
        .gap(start::HERO_HEAD_GAP)
        .min_w_0()
        .child(icon(Icon::Folder, start::HERO_ICON, colors.folder))
        .child(div().flex_1().min_w_0().child(titles(
            name_of(&folder.path),
            subtitle,
            text::HERO_NAME,
            colors,
        )))
}

/// A card's name in bold and the dim line under it, each cut to fit.
fn titles(
    name: String,
    subtitle: String,
    name_size: gpui::Pixels,
    colors: &Colors,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .min_w_0()
        .gap(start::SUBTITLE_GAP)
        .child(
            div()
                .truncate()
                .text_size(name_size)
                .font_weight(text::STRONG_WEIGHT)
                .text_color(colors.text)
                .child(name),
        )
        .child(
            div()
                .truncate()
                .text_size(text::LABEL)
                .font_weight(text::DETAIL_WEIGHT)
                .text_color(colors.text_dim)
                .child(subtitle),
        )
}

/// The large card's first entries, each with its size or item count, and
/// how many more the folder holds.
fn peek(folder: &RecentFolder, colors: &Colors) -> impl IntoElement {
    let rows = folder.peek.iter().map(|entry| {
        div()
            .flex()
            .items_center()
            .gap(start::PEEK_GAP)
            .h(start::PEEK_ROW)
            .min_w_0()
            .text_size(text::DETAIL)
            .child(entry_icon(&entry.name, entry.kind, None, colors))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(SharedString::from(
                        entry.name.to_string_lossy().into_owned(),
                    )),
            )
            .child(
                div()
                    .flex_none()
                    .font_weight(text::DETAIL_WEIGHT)
                    .text_color(colors.text_dim)
                    .child(amount(entry)),
            )
    });
    let more = more_than_peeked(folder);
    div()
        .flex()
        .flex_col()
        .pt(start::PEEK_TOP)
        .border_t(size::LINE)
        .border_color(colors.line)
        .min_w_0()
        .children(rows)
        .when_some(more, |list, more| {
            list.child(
                div()
                    .pt(start::SUBTITLE_GAP)
                    .pl(size::ROW_ICON + start::PEEK_GAP)
                    .text_size(text::LABEL)
                    .font_weight(text::DETAIL_WEIGHT)
                    .text_color(colors.text_dim)
                    .child(more),
            )
        })
}

/// A small card's fan: up to three entries, the outer two leaning out.
fn fan(entries: &[Peeked], colors: &Colors) -> impl IntoElement {
    let shown = entries.iter().take(FAN_ENTRIES);
    let count = shown.len();
    div()
        .flex()
        .flex_none()
        .children(shown.enumerate().map(|(index, entry)| {
            let (glyph, tint) = match entry.kind {
                EntryKind::Folder => (Icon::Folder, colors.folder),
                EntryKind::File => (Icon::File, colors.text_soft),
            };
            svg()
                .path(glyph.path())
                .flex_none()
                .size(start::FAN_ICON)
                .when(index > 0, |glyph| glyph.ml(-start::FAN_OVERLAP))
                .text_color(gpui::Hsla::from(tint))
                .with_transformation(Transformation::rotate(lean(index, count)))
        }))
}

/// How far fan entry `index` of `count` leans: the first left, the last
/// right, any between upright.
fn lean(index: usize, count: usize) -> Radians {
    let degrees = if count < FAN_ENTRIES {
        0.0
    } else if index == 0 {
        -start::FAN_TILT
    } else if index + 1 == count {
        start::FAN_TILT
    } else {
        0.0
    };
    Radians(degrees.to_radians())
}

/// A folder's name: its last part, or the whole path for the root.
fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.to_string_lossy().into_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Where a folder is: its parent, with home written `~`.
fn where_is(path: &Path, home: &Path) -> String {
    let parent = path.parent().map_or_else(PathBuf::new, Path::to_path_buf);
    match parent.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => HOME_MARK.to_owned(),
        Ok(rest) => format!("{HOME_MARK}/{}", rest.to_string_lossy()),
        Err(_) => parent.to_string_lossy().into_owned(),
    }
}

/// A peeked entry's size, or how many entries a folder holds.
fn amount(entry: &Peeked) -> String {
    match (entry.kind, entry.amount) {
        (_, None) => String::new(),
        (EntryKind::File, Some(bytes)) => columns::size(bytes),
        (EntryKind::Folder, Some(count)) => items(count),
    }
}

/// "1 item", "12 items".
fn items(count: u64) -> String {
    if count == 1 {
        format!("{count} item")
    } else {
        format!("{count} items")
    }
}

/// "3 more" when the folder holds more than its peek shows.
fn more_than_peeked(folder: &RecentFolder) -> Option<String> {
    let peeked = u64::try_from(folder.peek.len()).unwrap_or(u64::MAX);
    let more = folder.entries.saturating_sub(peeked);
    (more > 0).then(|| format!("{more} more"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(entries: u64, peeked: usize) -> RecentFolder {
        RecentFolder {
            path: PathBuf::from("/home/me/Projects/nova"),
            modified: None,
            entries,
            peek: vec![
                Peeked {
                    name: "a".into(),
                    kind: EntryKind::File,
                    amount: Some(1),
                };
                peeked
            ],
        }
    }

    #[test]
    fn where_a_folder_is_reads_home_as_a_tilde() {
        let home = Path::new("/home/me");
        assert_eq!(
            where_is(Path::new("/home/me/Projects/nova"), home),
            "~/Projects"
        );
        assert_eq!(where_is(Path::new("/home/me/Projects"), home), "~");
        assert_eq!(where_is(Path::new("/mnt/games/steam"), home), "/mnt/games");
        assert_eq!(where_is(Path::new("/home/meg/x"), home), "/home/meg");
        assert_eq!(where_is(Path::new("/"), home), "");
        assert_eq!(name_of(Path::new("/")), "/");
        assert_eq!(name_of(Path::new("/home/me/Projects")), "Projects");
    }

    #[test]
    fn a_peek_says_sizes_counts_and_what_is_left() {
        let file = |amount| Peeked {
            name: "f".into(),
            kind: EntryKind::File,
            amount,
        };
        let sub = |amount| Peeked {
            name: "d".into(),
            kind: EntryKind::Folder,
            amount,
        };
        assert_eq!(amount(&file(Some(1_800_000))), "1.8 MB");
        assert_eq!(amount(&sub(Some(1))), "1 item");
        assert_eq!(amount(&sub(Some(12))), "12 items");
        assert_eq!(amount(&sub(Some(0))), "0 items");
        assert_eq!(amount(&file(None)), "");
        assert_eq!(more_than_peeked(&folder(8, 5)).as_deref(), Some("3 more"));
        assert_eq!(more_than_peeked(&folder(5, 5)), None);
        assert_eq!(more_than_peeked(&folder(0, 0)), None);
    }

    #[test]
    fn a_full_fan_leans_its_outer_entries_out() {
        let tilt = start::FAN_TILT.to_radians();
        assert_eq!(lean(0, 3), Radians(-tilt));
        assert_eq!(lean(1, 3), Radians(0.0));
        assert_eq!(lean(2, 3), Radians(tilt));
        assert_eq!(lean(0, 2), Radians(0.0));
        assert_eq!(lean(1, 2), Radians(0.0));
    }
}
