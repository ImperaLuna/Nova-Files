//! The Start page's Drives group: one card per mounted drive, in the
//! sidebar's order, with its name, how much is free of how much, how full
//! it is as a large percent, and one bar. A drive nearly full shows its
//! percent and bar red. A click opens the drive. Cards carry the debug
//! selectors `drive-card-<n>`, counted from 0 over the cards shown.

use gpui::{
    AnyElement, ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _, relative,
};
use nova_files_api::DiskSpace;

use super::titles;
use crate::assets::Icon;
use crate::columns;
use crate::drives::{NEARLY_FULL, ShownDrive};
use crate::theme::{Colors, radius, size, start, text};
use crate::view::row::icon;
use crate::window::MainWindow;

/// The drive cards sit three to a row.
const PER_ROW: usize = 3;
/// A whole drive, in percent.
const WHOLE: f32 = 100.0;

impl MainWindow {
    /// The drive cards, three to a row, or `None` when no drive is
    /// mounted.
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
            let cards = drives
                .iter()
                .enumerate()
                .map(|(column, drive)| self.drive_card(row * PER_ROW + column, drive, &colors, cx));
            // Empty slots keep a short last row's cards as wide as the rest.
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

    /// One drive's card.
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
        let tint = if percent.is_some_and(|percent| percent >= NEARLY_FULL) {
            colors.full
        } else {
            colors.folder
        };
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
            .on_click(cx.listener(move |this, _: &ClickEvent, _, _| {
                this.open_drive(clicked.clone());
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
                                .text_color(if percent >= NEARLY_FULL {
                                    colors.full
                                } else {
                                    colors.text
                                })
                                .child(format!("{percent}%")),
                        )
                    }),
            )
            .child(
                div()
                    .h(start::BAR)
                    .rounded(radius::SMALL)
                    .bg(colors.press)
                    .overflow_hidden()
                    .when_some(percent, |bar, percent| {
                        bar.child(
                            div()
                                .h_full()
                                .w(relative(f32::from(percent) / WHOLE))
                                .rounded(radius::SMALL)
                                .bg(tint),
                        )
                    }),
            )
            .into_any_element()
    }
}

/// "381 GB free of 1.0 TB".
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
}
