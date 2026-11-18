//! The Places section: the Start page first, then the standard folders
//! and the pinned ones, as `places` lists them, then Projects and Games,
//! then the trash with how many items it holds. Each listed folder's row
//! carries the debug selector `place-<n>`, counted from 0 in the order
//! shown; the others carry `place-start`, `place-projects`,
//! `place-games` and `place-trash`. A picker shows only
//! the folders: it has no Start page, Games and Projects aren't
//! folders, and its trash isn't one to pick from. In a main window, the + (`pin`) on
//! the head, or at the end of the row in a narrow window, picks a folder
//! to pin, and right-click on a pinned folder opens
//! its menu (see `window/pin`). Files dropped on a folder's row go into
//! it, and on the trash's to the trash (see `drop`); anything dragged and
//! held over a row opens it.

use gpui::{
    ClickEvent, Context, Div, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent,
    ParentElement as _, Stateful, StatefulInteractiveElement as _, Styled as _, div,
    prelude::FluentBuilder as _,
};

use super::{Row, new_tab_on_middle_click, on_middle_click, row, section_button, section_head};
use crate::assets::Icon;
use nova_files_api::Gathering;

use crate::location::Location;
use crate::places::{self, START_LABEL, TRASH_LABEL};
use crate::theme::{Colors, size};
use crate::window::{Aim, HoverOpens, MainWindow};

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
                let is_here = place.is_here(here);
                let aimed = place.name.clone();
                row(
                    Row {
                        id: ("place", index).into(),
                        selector: format!("place-{index}"),
                        glyph: place.icon,
                        name: place.name,
                        number: None,
                        here: is_here,
                        pressed: false,
                    },
                    narrow,
                    &colors,
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, _| {
                    this.open(&target);
                }))
                .map(|row| new_tab_on_middle_click(row, place.path.clone(), main, cx))
                .map(|row| self.drop_target(row, Aim::folder(place.path.clone(), aimed), cx))
                .map(|row| {
                    let opens = HoverOpens::Place(Location::Folder(place.path.clone()));
                    Self::opens_on_hover(row, opens, cx)
                })
                .when_some(pinned, |row, path| {
                    row.on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            this.open_pinned_menu(path.clone(), event.position, cx);
                        }),
                    )
                })
            });
        let start = main.then(|| self.start_row(narrow, &colors, cx));
        let gathered = Gathering::ALL
            .into_iter()
            .filter(|_| main)
            .map(|place| self.gathered_row(place, narrow, &colors, cx));
        let trash = main.then(|| self.trash_row(narrow, &colors, cx));
        let pin = main.then(|| {
            section_button("pin", Icon::Plus, &colors)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.pick_folder_to_pin(cx)))
        });
        let list = div()
            .flex()
            .gap(size::PLACE_GAP)
            .children(start)
            .children(rows)
            .children(gathered)
            .children(trash);
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

    /// The Start page's row, first in Places.
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

    /// The row of `place`, Projects or Games, before the trash's.
    fn gathered_row(
        &self,
        place: Gathering,
        narrow: bool,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let (name, glyph) = places::gathering_look(place);
        let selector = format!("place-{}", name.to_lowercase());
        row(
            Row {
                id: selector.clone().into(),
                selector,
                glyph,
                name: name.into(),
                number: None,
                here: self.gathered_here() == Some(place),
                pressed: false,
            },
            narrow,
            colors,
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.open_gathered(place, cx);
        }))
        .map(|row| {
            on_middle_click(row, true, cx, move |this, cx| {
                this.new_tab(&Location::Gathered(place), cx);
            })
        })
    }

    /// The trash's row, last in Places, with how many items it holds.
    fn trash_row(&self, narrow: bool, colors: &Colors, cx: &Context<Self>) -> Stateful<Div> {
        let count = self
            .trash_count()
            .filter(|count| *count > 0)
            .map(|count| (count.to_string().into(), colors.text_dim));
        row(
            Row {
                id: "place-trash".into(),
                selector: "place-trash".to_owned(),
                glyph: Icon::Trash,
                name: TRASH_LABEL.into(),
                number: count,
                here: self.trash_open(),
                pressed: false,
            },
            narrow,
            colors,
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_trash(cx)))
        .map(|row| on_middle_click(row, true, cx, Self::open_trash_in_new_tab))
        .map(|row| self.drop_target(row, Aim::trash(), cx))
        .map(|row| {
            let opens = HoverOpens::Place(Location::Folder(self.trash_folder().to_path_buf()));
            Self::opens_on_hover(row, opens, cx)
        })
    }
}
