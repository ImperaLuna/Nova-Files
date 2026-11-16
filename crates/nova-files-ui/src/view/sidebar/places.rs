//! The Places section: the standard folders and the pinned ones, as
//! `places` lists them, then, outside a picker, the trash with how many
//! items it holds. Each folder's row carries the debug selector
//! `place-<n>`, counted from 0 in the order shown, and the trash's
//! `place-trash`. Files dropped on a row go into it (see `drop`). In a
//! main window, the + (`pin`) on the head, or at the end of the row in a
//! narrow window, picks a folder to pin, and right-click on a pinned
//! folder opens its menu (see `window/pin`).

use gpui::{
    ClickEvent, Context, Div, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent,
    ParentElement as _, Stateful, StatefulInteractiveElement as _, Styled as _, div,
    prelude::FluentBuilder as _,
};

use super::{Row, row, section_button, section_head};
use crate::assets::Icon;
use crate::dropping::DropAt;
use crate::places;
use crate::theme::{Colors, size};
use crate::window::MainWindow;

const LABEL: &str = "Places";
const TRASH_SELECTOR: &str = "place-trash";

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
                .map(|row| self.drop_target(row, DropAt::Folder(place.path.clone()), cx))
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
        let list = div()
            .flex()
            .gap(size::PLACE_GAP)
            .children(rows)
            .when(main, |list| {
                list.child(self.trash_place(narrow, &colors, cx))
            });
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

impl MainWindow {
    /// The trash, after the folders: how many items it holds, once the
    /// core has said; files dropped on it go to it.
    fn trash_place(&self, narrow: bool, colors: &Colors, cx: &Context<Self>) -> Stateful<Div> {
        let number = self
            .trash_count()
            .filter(|&count| count > 0)
            .map(|count| (count.to_string().into(), colors.text_dim));
        let trash = row(
            Row {
                id: TRASH_SELECTOR.into(),
                selector: TRASH_SELECTOR.to_owned(),
                glyph: Icon::Trash,
                name: places::TRASH_LABEL.into(),
                number,
                here: self.trash_open(),
            },
            narrow,
            colors,
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_trash(cx)));
        self.drop_target(trash, DropAt::Trash, cx)
    }
}
