//! Draws the main window: a toolbar with Up and the folder's path, the
//! places column, and the folder's entries.
//!
//! The entries are a `uniform_list`, which builds only the rows on screen,
//! so a folder of any size costs the same to draw. Each row is an icon slot
//! and a name. Every element a test clicks carries a debug selector:
//! `up`, `place-<n>` and `entry-<n>`, counted from 0.

use gpui::{
    AnyElement, ClickEvent, Context, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
    div, prelude::FluentBuilder as _, rgb, svg, uniform_list,
};
use nova_files_api::{EntryKind, Place, PlaceKind};

use crate::assets::Icon;
use crate::theme::{color, radius, size};
use crate::window::MainWindow;

impl Render for MainWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(color::SURFACE))
            .text_color(rgb(color::TEXT))
            .text_size(size::TEXT)
            .child(self.toolbar(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.places_column(cx))
                    .child(self.entries(cx)),
            )
    }
}

impl MainWindow {
    fn toolbar(&self, cx: &Context<Self>) -> impl IntoElement {
        let can_go_up = self.parent().is_some();
        let tone = if can_go_up {
            color::TEXT
        } else {
            color::TEXT_DIM
        };
        let up = div()
            .id("up")
            .debug_selector(|| "up".to_owned())
            .flex()
            .items_center()
            .gap(size::BUTTON_GAP)
            .h(size::ROW_HEIGHT)
            .px(size::PAD)
            .rounded(radius::BUTTON)
            .border(size::DIVIDER)
            .border_color(rgb(color::DIVIDER))
            .text_color(rgb(tone))
            .child(icon(Icon::Up, size::BUTTON_ICON, rgb(tone).into()))
            .child("Up")
            .when(can_go_up, |button| {
                button
                    .cursor_pointer()
                    .hover(|button| button.bg(rgb(color::ROW_HIGHLIGHT)))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, _| this.go_up()))
            });
        let path = self
            .folder()
            .map(|folder| SharedString::from(folder.to_string_lossy().into_owned()))
            .unwrap_or_default();
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(size::GAP)
            .h(size::TOOLBAR_HEIGHT)
            .px(size::PAD)
            .bg(rgb(color::SURFACE_SUNK))
            .border_b(size::DIVIDER)
            .border_color(rgb(color::DIVIDER))
            .child(up)
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(size::TEXT_SMALL)
                    .text_color(rgb(color::TEXT_DIM))
                    .child(path),
            )
    }

    fn places_column(&self, cx: &Context<Self>) -> impl IntoElement {
        let open = self.folder();
        let rows: Vec<AnyElement> = self
            .places()
            .iter()
            .enumerate()
            .map(|(index, place)| {
                let is_open = open == Some(place.path.as_path());
                let target = place.path.clone();
                row(("place", index), format!("place-{index}"))
                    .when(is_open, |row| row.bg(rgb(color::ROW_HIGHLIGHT)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, _| {
                        this.open(&target);
                    }))
                    .child(icon(
                        Icon::Folder,
                        size::ROW_ICON,
                        rgb(color::FOLDER).into(),
                    ))
                    .child(place_name(place))
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(size::ROW_GAP)
            .w(size::PLACES_WIDTH)
            .p(size::PAD)
            .bg(rgb(color::SURFACE_SUNK))
            .border_r(size::DIVIDER)
            .border_color(rgb(color::DIVIDER))
            .children(rows)
    }

    fn entries(&self, cx: &Context<Self>) -> impl IntoElement {
        let count = self
            .listing
            .as_ref()
            .map_or(0, |listing| listing.entries.len());
        uniform_list(
            "entries",
            count,
            cx.processor(|this, range: std::ops::Range<usize>, _window, cx| {
                let Some(listing) = &this.listing else {
                    return Vec::new();
                };
                listing
                    .entries
                    .get(range)
                    .unwrap_or_default()
                    .iter()
                    .map(|entry| {
                        let id = entry.id;
                        let name = SharedString::from(entry.name.to_string_lossy().into_owned());
                        let index = usize::try_from(id.0).unwrap_or_default();
                        let row = row(("entry", index), format!("entry-{index}"))
                            .child(kind_icon(entry.kind))
                            .child(div().min_w_0().truncate().child(name));
                        match entry.kind {
                            EntryKind::Folder => row
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, _| {
                                    this.open_entry(id);
                                }))
                                .into_any_element(),
                            EntryKind::File => row.into_any_element(),
                        }
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.scroll)
        .flex_1()
        .h_full()
        .p(size::PAD)
    }
}

/// One row of either list: icon slot and label, highlighted under the
/// pointer.
fn row(id: (&'static str, usize), selector: String) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .debug_selector(|| selector)
        .flex()
        .flex_none()
        .items_center()
        .gap(size::GAP)
        .h(size::ROW_HEIGHT)
        .px(size::PAD)
        .rounded(radius::ROW)
        .hover(|row| row.bg(rgb(color::ROW_HIGHLIGHT)))
}

/// The icon slot of an entry row; thumbnails take its place later.
fn kind_icon(kind: EntryKind) -> impl IntoElement {
    match kind {
        EntryKind::Folder => icon(Icon::Folder, size::ROW_ICON, rgb(color::FOLDER).into()),
        EntryKind::File => icon(Icon::File, size::ROW_ICON, rgb(color::FILE).into()),
    }
}

fn icon(icon: Icon, side: gpui::Pixels, tint: Hsla) -> impl IntoElement {
    svg()
        .path(icon.path())
        .flex_none()
        .size(side)
        .text_color(tint)
}

/// A place's label: "Home" for the home folder, otherwise the folder's own
/// name, which follows the user's language ("Dokumente", "Images").
fn place_name(place: &Place) -> SharedString {
    match place.kind {
        PlaceKind::Home => SharedString::from(HOME_LABEL),
        PlaceKind::Documents | PlaceKind::Pictures | PlaceKind::Downloads => place
            .path
            .file_name()
            .map(|name| SharedString::from(name.to_string_lossy().into_owned()))
            .unwrap_or_default(),
    }
}

const HOME_LABEL: &str = "Home";

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn places_are_named_by_their_folder_except_home() {
        let place = |kind, path: &str| Place {
            kind,
            path: PathBuf::from(path),
        };
        assert_eq!(place_name(&place(PlaceKind::Home, "/home/me")), "Home");
        assert_eq!(
            place_name(&place(PlaceKind::Documents, "/home/me/Dokumente")),
            "Dokumente"
        );
    }
}
