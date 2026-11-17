//! The Drives section, fixed at the bottom of the sidebar: each drive as
//! `drives` lists it, with how much is free as a number on the right, red
//! when nearly full. A click opens it (mounting it first when needed);
//! middle-click opens it in a new tab; right-click opens its menu, which
//! has Eject for a drive that can be unplugged. There is no eject button. Rows carry the debug selectors
//! `drive-<n>`, counted from 0.

use gpui::{
    ClickEvent, Context, Div, InteractiveElement as _, MouseButton, MouseDownEvent,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, div,
    prelude::FluentBuilder as _,
};

use super::{Row, on_middle_click, row, section_head};
use crate::assets::Icon;
use crate::drives;
use crate::theme::{Colors, size};
use crate::window::{Aim, MainWindow};

const LABEL: &str = "Drives";

impl MainWindow {
    /// The Drives section; without its head when `narrow`.
    pub(super) fn drives_section(&self, narrow: bool, cx: &Context<Self>) -> Div {
        let colors = Colors::of(cx);
        let shown = self.shown_drives();
        let here = self
            .folder()
            .and_then(|folder| drives::here(&shown, folder, &self.home));
        let main = self.pick.is_none();
        let rows = shown.into_iter().enumerate().map(|(index, drive)| {
            let (clicked, menu, in_new_tab) = (drive.clone(), drive.clone(), drive.clone());
            let aimed = drive.name.clone();
            let number = drive
                .folder
                .as_deref()
                .and_then(|folder| self.disk_space(folder))
                .map(|space| {
                    let (text, full) = drives::free(space);
                    (text, if full { colors.full } else { colors.text_dim })
                })
                // A narrow window keeps its drives to their names.
                .filter(|_| !narrow);
            let row = row(
                Row {
                    id: ("drive", index).into(),
                    selector: format!("drive-{index}"),
                    glyph: Icon::Drive,
                    name: drive.name,
                    number,
                    here: here == Some(index),
                    pressed: false,
                },
                narrow,
                &colors,
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, _| {
                this.open_drive(clicked.clone());
            }))
            .when_some(drive.folder, |row, folder| {
                self.drop_target(row, Aim::folder(folder, aimed), cx)
            });
            if main {
                on_middle_click(row, main, cx, move |this, cx| {
                    this.open_drive_in_new_tab(in_new_tab.clone(), cx);
                })
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        this.open_drive_menu(menu.clone(), event.position, cx);
                    }),
                )
            } else {
                row
            }
        });
        let list = div().flex().gap(size::PLACE_GAP).children(rows);
        if narrow {
            return list;
        }
        // Many drives scroll in their own part, so Places keep their room.
        div()
            .flex()
            .flex_col()
            .min_h_0()
            .pb(size::GAP)
            .border_t(size::LINE)
            .border_color(colors.line)
            .child(section_head(LABEL, None, &colors))
            .child(
                list.id("drives-scroll")
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll(),
            )
    }
}
