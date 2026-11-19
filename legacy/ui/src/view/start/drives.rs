use gpui::{
    AnyElement, ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _, relative,
};
use nova_files_drives_api::DiskSpace;

use super::titles;
use crate::assets::Icon;
use crate::columns;
use crate::theme::{Colors, radius, size, start, text};
use crate::view::row::icon;
use crate::window::MainWindow;
use nova_files_drives_ui::{NEARLY_FULL, ShownDrive};

const PER_ROW: usize = 3;
const WHOLE: f32 = 100.0;

impl MainWindow {
    pub(super) fn drive_cards(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let mounted: Vec<ShownDrive> = self
            .shown_drives()
            .into_iter()
            .filter(|drive| drive.folder.is_some())
            .collect();
        if mounted.is_empty() {
            return None;
        }
        let colors = Colors::of(cx);
        let rows = mounted.chunks(PER_ROW).enumerate().map(|(row, drives)| {
            let cards = drives.iter().enumerate().map(|(column, drive)| {
                self.drive_card(card_index(row, column), drive, &colors, cx)
            });
            let fillers = (drives.len()..PER_ROW).map(|_| div().flex_1().into_any_element());
            div()
                .flex()
                .gap(size::GAP)
                .children(cards)
                .children(fillers)
        });
        Some(
            div()
                .flex()
                .flex_col()
                .gap(size::GAP)
                .children(rows)
                .into_any_element(),
        )
    }

    fn drive_card(
        &self,
        index: usize,
        drive: &ShownDrive,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> AnyElement {
        let space = drive
            .folder
            .as_deref()
            .and_then(|folder| self.disk_space(folder));
        let percent = space.map(DiskSpace::percent_used);
        let full = nearly_full(percent);
        let tint = if full { colors.full } else { colors.folder };
        let clicked = drive.clone();
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
            .hover(|card| card.bg(colors.lifted))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.open_drive(clicked.clone(), cx);
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
            .child(bar(percent.map(filled), tint, colors))
            .into_any_element()
    }
}

pub(in crate::view) fn bar(share: Option<f32>, tint: gpui::Rgba, colors: &Colors) -> AnyElement {
    div()
        .h(start::BAR)
        .rounded(radius::SMALL)
        .bg(colors.press)
        .overflow_hidden()
        .when_some(share, |bar, share| {
            bar.child(
                div()
                    .h_full()
                    .w(relative(share))
                    .rounded(radius::SMALL)
                    .bg(tint),
            )
        })
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
        columns::size(space.free),
        columns::size(space.total)
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
