//! The Places section: the standard folders and the pinned ones, as
//! `places` lists them. Each row carries the debug selector `place-<n>`,
//! counted from 0 in the order shown. In a main window, the + (`pin`) on
//! the head, or at the end of the row in a narrow window, picks a folder
//! to pin, and right-click on a pinned folder opens
//! its menu (see `window/pin`).

use gpui::{
    ClickEvent, Context, Div, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, div,
    prelude::FluentBuilder as _,
};

use super::{Row, row, section_button, section_head};
use crate::assets::Icon;
use crate::places;
use crate::theme::{Colors, size};
use crate::window::MainWindow;

const LABEL: &str = "Places";

impl MainWindow {
    /// The Places section; without its head when `narrow`.
    pub(super) fn places_section(&self, narrow: bool, cx: &Context<Self>) -> Div {
        let colors = Colors::of(cx);
        let here = self.folder();
        let main = self.pick.is_none();
        let rows = places::shown(self.places(), self.pinned())
            .into_iter()
            .enumerate()
            .map(|(index, place)| {
                let target = place.path.clone();
                let pinned = (main && place.pinned).then(|| place.path.clone());
                row(
                    Row {
                        id: ("place", index).into(),
                        selector: format!("place-{index}"),
                        glyph: place.icon,
                        name: place.name,
                        number: None,
                        here: here == Some(place.path.as_path()),
                    },
                    narrow,
                    &colors,
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, _| {
                    this.open(&target);
                }))
                .when_some(pinned, |row, path| {
                    row.on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            this.open_pinned_menu(path.clone(), event.position, cx);
                        }),
                    )
                })
            });
        let pin = main.then(|| {
            section_button("pin", Icon::Plus, &colors)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.pick_folder_to_pin(cx)))
        });
        let list = div().flex().gap(size::PLACE_GAP).children(rows);
        if narrow {
            // No head to carry the +: it ends the row, so pinning stays
            // within reach.
            return list.items_center().children(pin);
        }
        div()
            .flex()
            .flex_col()
            .child(section_head(
                LABEL,
                pin.map(IntoElement::into_any_element),
                &colors,
            ))
            .child(list.flex_col())
    }
}
