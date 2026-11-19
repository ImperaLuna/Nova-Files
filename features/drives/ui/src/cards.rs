use gpui::{
    AnyElement, ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _,
};
use nova_files_drives_api::DiskSpace;
use nova_files_shell::assets::Icon;
use nova_files_shell::format;
use nova_files_shell::theme::{Colors, radius, size, start, text};
use nova_files_shell::widgets::{card_group, icon, space_bar, titles};

use crate::{DrivesPart, NEARLY_FULL, ShownDrive};

const DRIVES_LABEL: &str = "Drives";
const PER_ROW: usize = 3;
const WHOLE: f32 = 100.0;

pub fn drive_cards(part: &DrivesPart, cx: &Context<DrivesPart>) -> Option<AnyElement> {
    let mounted: Vec<ShownDrive> = part
        .shown()
        .into_iter()
        .filter(|drive| drive.folder.is_some())
        .collect();
    if mounted.is_empty() {
        return None;
    }
    let colors = Colors::of(cx);
    let rows = mounted.chunks(PER_ROW).enumerate().map(|(row, drives)| {
        let cards = drives
            .iter()
            .enumerate()
            .map(|(column, drive)| drive_card(part, card_index(row, column), drive, &colors, cx));
        let fillers = (drives.len()..PER_ROW).map(|_| div().flex_1().into_any_element());
        div()
            .flex()
            .gap(size::GAP)
            .children(cards)
            .children(fillers)
    });
    let cards = div()
        .flex()
        .flex_col()
        .gap(size::GAP)
        .children(rows)
        .into_any_element();
    Some(card_group(DRIVES_LABEL, cards, &colors).into_any_element())
}

fn drive_card(
    part: &DrivesPart,
    index: usize,
    drive: &ShownDrive,
    colors: &Colors,
    cx: &Context<DrivesPart>,
) -> AnyElement {
    let space = drive
        .folder
        .as_deref()
        .and_then(|folder| part.disk_space(folder));
    let percent = space.map(DiskSpace::percent_used);
    let full = nearly_full(percent);
    let tint = if full { colors.full } else { colors.folder };
    let clicked = drive.clone();
    let lifted = colors.lifted;
    div()
        .id(("drive-card", index))
        .debug_selector(move || format!("drive-card-{index}"))
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(start::CARD_GAP)
        .p(start::CARD_PAD)
        .rounded(radius::CARD)
        .bg(colors.raised)
        .cursor_pointer()
        .hover(move |card| card.bg(lifted))
        .on_click(cx.listener(move |part, _: &ClickEvent, _, cx| {
            part.open_drive(clicked.clone(), cx);
        }))
        .child(
            div()
                .flex()
                .items_center()
                .gap(start::DRIVE_HEAD_GAP)
                .min_w_0()
                .child(icon(Icon::Drive, start::DRIVE_ICON, colors.text_dim))
                .child(div().flex_1().min_w_0().child(titles(
                    drive.name.to_string(),
                    space.map(free_of).unwrap_or_default(),
                    text::CARD_NAME,
                    colors,
                )))
                .when_some(percent, |head, percent| {
                    head.child(
                        div()
                            .flex_none()
                            .text_size(text::PERCENT)
                            .font_weight(text::STRONG_WEIGHT)
                            .text_color(if full { colors.full } else { colors.text })
                            .child(format!("{percent}%")),
                    )
                }),
        )
        .child(space_bar(percent.map(filled), tint, colors))
        .into_any_element()
}

const fn card_index(row: usize, column: usize) -> usize {
    row * PER_ROW + column
}

fn nearly_full(percent: Option<u8>) -> bool {
    percent.is_some_and(|percent| percent >= NEARLY_FULL)
}

fn filled(percent: u8) -> f32 {
    f32::from(percent) / WHOLE
}

fn free_of(space: DiskSpace) -> String {
    format!(
        "{} free of {}",
        format::size(space.free),
        format::size(space.total)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drive_says_what_is_free_of_its_size() {
        let space = DiskSpace {
            total: 1_000_000_000_000,
            free: 381_000_000_000,
        };
        assert_eq!(free_of(space), "381 GB free of 1.0 TB");
    }

    #[test]
    fn cards_count_on_across_rows() {
        assert_eq!(card_index(0, 0), 0);
        assert_eq!(card_index(0, 2), 2);
        assert_eq!(card_index(1, 0), PER_ROW);
        assert_eq!(card_index(2, 1), 2 * PER_ROW + 1);
    }

    #[test]
    fn a_drive_turns_red_from_nearly_full() {
        assert!(!nearly_full(None));
        assert!(!nearly_full(Some(NEARLY_FULL - 1)));
        assert!(nearly_full(Some(NEARLY_FULL)));
        assert!(nearly_full(Some(100)));
    }

    #[test]
    fn the_bar_fills_as_much_as_the_drive_is_used() {
        assert!((filled(0) - 0.0).abs() < f32::EPSILON);
        assert!((filled(25) - 0.25).abs() < f32::EPSILON);
        assert!((filled(100) - 1.0).abs() < f32::EPSILON);
    }
}
