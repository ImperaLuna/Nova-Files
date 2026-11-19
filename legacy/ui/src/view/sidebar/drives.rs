use gpui::{
    ClickEvent, Context, Div, InteractiveElement as _, MouseButton, MouseDownEvent,
    ParentElement as _, Stateful, StatefulInteractiveElement as _, Styled as _, div,
    prelude::FluentBuilder as _,
};

use nova_files_shell::{SidebarAt, SidebarRow};

use super::{Row, on_middle_click, row, section_head};
use crate::assets::Icon;
use crate::drives::{self, ShownDrive};
use crate::location::Location;
use crate::theme::{Colors, size};
use crate::window::{Aim, HoverOpens, MainWindow};

const LABEL: &str = "Drives";

impl MainWindow {
    pub(super) fn drives_section(&self, narrow: bool, cx: &Context<Self>) -> Div {
        let colors = Colors::of(cx);
        let shown = self.shown_drives();
        let here = self
            .folder()
            .and_then(|folder| drives::here(&shown, folder, &self.home));
        let rows = shown
            .into_iter()
            .enumerate()
            .map(|(index, drive)| {
                let row = self.drive_row((index, drive), here == Some(index), narrow, cx);
                (SidebarRow::OWN, row)
            })
            .chain(self.part_rows(SidebarAt::Drives, narrow, &colors, cx));
        let mut ranked: Vec<(u8, Stateful<Div>)> = rows.collect();
        ranked.sort_by_key(|(rank, _)| *rank);
        let list = div()
            .flex()
            .gap(size::PLACE_GAP)
            .children(ranked.into_iter().map(|(_, row)| row));
        if narrow {
            return list;
        }
        div()
            .flex()
            .flex_col()
            .min_h_0()
            .pb(size::GAP)
            .border_t(size::LINE)
            .border_color(colors.line)
            .child(section_head(
                LABEL,
                self.part_buttons(SidebarAt::Drives, &colors, cx),
                &colors,
            ))
            .child(
                list.id("drives-scroll")
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll(),
            )
    }

    fn drive_row(
        &self,
        (index, drive): (usize, ShownDrive),
        here: bool,
        narrow: bool,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let colors = &Colors::of(cx);
        let main = self.pick.is_none();
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
            .filter(|_| !narrow);
        let row = row(
            Row {
                id: ("drive", index).into(),
                selector: format!("drive-{index}"),
                glyph: Icon::Drive,
                name: drive.name,
                number,
                here,
                pressed: false,
                lead: None,
            },
            narrow,
            colors,
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, _, _| {
            this.open_drive(clicked.clone());
        }))
        .when_some(drive.folder, |row, folder| {
            let opens = HoverOpens::Place(Location::Folder(folder.clone()));
            let row = self.drop_target(row, Aim::folder(folder, aimed), cx);
            Self::opens_on_hover(row, opens, cx)
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
    }
}
