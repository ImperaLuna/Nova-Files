//! The Places section: the standard folders and the pinned ones, as
//! `places` lists them. Each row carries the debug selector `place-<n>`,
//! counted from 0 in the order shown.

use gpui::{
    ClickEvent, Context, Div, ParentElement as _, StatefulInteractiveElement as _, Styled as _, div,
};

use super::{Row, row, section_head};
use crate::places;
use crate::theme::{Colors, size};
use crate::window::MainWindow;

const LABEL: &str = "Places";

impl MainWindow {
    /// The Places section; without its head when `narrow`.
    pub(super) fn places_section(&self, narrow: bool, cx: &Context<Self>) -> Div {
        let colors = Colors::of(cx);
        let here = self.folder();
        let rows = places::shown(self.places(), self.pinned())
            .into_iter()
            .enumerate()
            .map(|(index, place)| {
                let target = place.path.clone();
                let is_here = place.is_here(here);
                row(
                    Row {
                        id: ("place", index).into(),
                        selector: format!("place-{index}"),
                        glyph: place.icon,
                        name: place.name,
                        number: None,
                        here: is_here,
                    },
                    narrow,
                    &colors,
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, _| {
                    this.open(&target);
                }))
            });
        let list = div().flex().gap(size::PLACE_GAP).children(rows);
        if narrow {
            return list;
        }
        div()
            .flex()
            .flex_col()
            .child(section_head(LABEL, None, &colors))
            .child(list.flex_col())
    }
}
