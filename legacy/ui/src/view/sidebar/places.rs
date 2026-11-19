use gpui::{
    ClickEvent, Context, Div, ParentElement as _, Stateful, StatefulInteractiveElement as _,
    Styled as _, div, prelude::FluentBuilder as _,
};

use super::{Row, on_middle_click, row, section_head};
use crate::assets::Icon;

use nova_files_shell::{SidebarAt, SidebarRow};

use crate::location::Location;
use crate::places::START_LABEL;
use crate::theme::{Colors, size};
use crate::window::{HoverOpens, MainWindow};

const LABEL: &str = "Places";

impl MainWindow {
    pub(super) fn places_section(&self, narrow: bool, cx: &Context<Self>) -> Div {
        let colors = Colors::of(cx);
        let main = !self.picker();
        let start = main
            .then(|| (SidebarRow::START, self.start_row(narrow, &colors, cx)))
            .into_iter();
        let mut ranked: Vec<(u8, Stateful<Div>)> = start
            .chain(self.part_rows(SidebarAt::Places, narrow, &colors, cx))
            .collect();
        ranked.sort_by_key(|(rank, _)| *rank);
        let buttons = self.part_buttons(SidebarAt::Places, &colors, cx);
        let list = div()
            .flex()
            .gap(size::PLACE_GAP)
            .children(ranked.into_iter().map(|(_, row)| row));
        if narrow {
            return list.items_center().children(buttons);
        }
        div()
            .flex()
            .flex_col()
            .child(section_head(LABEL, buttons, &colors))
            .child(list.flex_col())
    }

    fn start_row(&self, narrow: bool, colors: &Colors, cx: &Context<Self>) -> Stateful<Div> {
        row(
            Row {
                id: "place-start".into(),
                selector: "place-start".to_owned(),
                glyph: Icon::Start,
                name: START_LABEL.into(),
                number: None,
                here: self.is_on_start(),
                pressed: false,
                lead: None,
            },
            narrow,
            colors,
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_start(cx)))
        .map(|row| Self::opens_on_hover(row, HoverOpens::Place(Location::Start), cx))
        .map(|row| {
            on_middle_click(row, true, cx, |this, cx| {
                this.new_tab(&Location::Start, cx);
            })
        })
    }
}
