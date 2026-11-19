use std::path::Path;

use gpui::{
    AnyElement, ClickEvent, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, div,
    prelude::FluentBuilder as _,
};
use nova_files_api::EntryKind;
use nova_files_shell::assets::Icon;
use nova_files_shell::format::{self, ITEM, ITEMS};
use nova_files_shell::kind::icon_kind;
use nova_files_shell::paths::{name_of, where_is};
use nova_files_shell::theme::{Colors, radius, size, start, text};
use nova_files_shell::widgets::{card_group, fan, icon, kind_icon, titles};
use nova_files_start_api::{Peeked, RecentFolder};

use crate::StartPart;

const RECENT_LABEL: &str = "Jump back in";
const SEPARATOR: &str = " · ";
const SMALL_PER_ROW: usize = 2;

pub fn recent(part: &StartPart, cx: &Context<StartPart>) -> Option<AnyElement> {
    let colors = Colors::of(cx);
    let mut folders = part.recent.iter().enumerate();
    let (_, newest) = folders.next()?;
    let home = &part.host.home;
    let mut small = folders
        .map(|(index, folder)| small_card(index, folder, home, &colors, cx))
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
    let cards = div()
        .flex()
        .gap(size::GAP)
        .child(
            card(0, &newest.path, &colors, cx)
                .flex_1()
                .p(start::HERO_PAD)
                .gap(start::HERO_GAP)
                .child(hero_head(newest, home, &colors))
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
        .into_any_element();
    Some(card_group(RECENT_LABEL, cards, &colors).into_any_element())
}

fn small_card(
    index: usize,
    folder: &RecentFolder,
    home: &Path,
    colors: &Colors,
    cx: &Context<StartPart>,
) -> AnyElement {
    card(index, &folder.path, colors, cx)
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
                .child(fan(folder.peek.iter().map(|peeked| peeked.kind), colors)),
        )
        .child(titles(
            name_of(&folder.path),
            where_is(&folder.path, home),
            text::CARD_NAME,
            colors,
        ))
        .into_any_element()
}

fn card(index: usize, folder: &Path, colors: &Colors, cx: &Context<StartPart>) -> Stateful<Div> {
    let (target, in_new_tab) = (folder.to_owned(), folder.to_owned());
    let lifted = colors.lifted;
    div()
        .id(("recent", index))
        .debug_selector(move || format!("recent-{index}"))
        .flex()
        .flex_col()
        .min_w_0()
        .rounded(radius::CARD)
        .bg(colors.raised)
        .cursor_pointer()
        .hover(move |card| card.bg(lifted))
        .on_click(cx.listener(move |part, _: &ClickEvent, _, cx| part.open(&target, cx)))
        .on_aux_click(cx.listener(move |part, event: &ClickEvent, _, cx| {
            if event.is_middle_click() {
                part.open_in_new_tab(&in_new_tab, cx);
            }
        }))
}

fn hero_head(folder: &RecentFolder, home: &Path, colors: &Colors) -> impl IntoElement {
    let subtitle = hero_subtitle(folder, home);
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

fn hero_subtitle(folder: &RecentFolder, home: &Path) -> String {
    let place = where_is(&folder.path, home);
    match folder.modified.map(format::modified) {
        Some(modified) if !modified.is_empty() => format!("{place}{SEPARATOR}{modified}"),
        Some(_) | None => place,
    }
}

fn peek(folder: &RecentFolder, colors: &Colors) -> impl IntoElement {
    let rows = folder.peek.iter().map(|entry| {
        div()
            .flex()
            .items_center()
            .gap(start::PEEK_GAP)
            .h(start::PEEK_ROW)
            .min_w_0()
            .text_size(text::DETAIL)
            .child(kind_icon(
                icon_kind(&entry.name, entry.kind),
                size::ROW_ICON,
                colors,
            ))
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
                    .pl(start::PEEK_MORE_INSET)
                    .text_size(text::LABEL)
                    .font_weight(text::DETAIL_WEIGHT)
                    .text_color(colors.text_dim)
                    .child(more),
            )
        })
}

fn amount(entry: &Peeked) -> String {
    match (entry.kind, entry.amount) {
        (_, None) => String::new(),
        (EntryKind::File, Some(bytes)) => format::size(bytes),
        (EntryKind::Folder, Some(count)) => {
            format::count(usize::try_from(count).unwrap_or(usize::MAX), ITEM, ITEMS)
        }
    }
}

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
            path: std::path::PathBuf::from("/home/me/Projects/nova"),
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
    fn the_large_card_says_when_its_folder_changed_when_known() {
        let home = Path::new("/home/me");
        let mut newest = folder(0, 0);
        assert_eq!(hero_subtitle(&newest, home), "~/Projects");
        let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        newest.modified = Some(time);
        assert_eq!(
            hero_subtitle(&newest, home),
            format!("~/Projects{SEPARATOR}{}", format::modified(time))
        );
        let unwritable = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1 << 62);
        assert_eq!(format::modified(unwritable), "");
        newest.modified = Some(unwritable);
        assert_eq!(hero_subtitle(&newest, home), "~/Projects");
    }

    #[test]
    fn the_more_line_lines_up_with_the_peeked_names() {
        assert_eq!(start::PEEK_MORE_INSET, size::ROW_ICON + start::PEEK_GAP);
    }
}
