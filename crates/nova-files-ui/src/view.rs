//! Draws the main window: a toolbar with Back, Forward, Up and the
//! folder's path, the places column, and the folder's entries under a
//! column header.
//!
//! The entries are a `uniform_list`, which builds only the rows on screen,
//! so a folder of any size costs the same to draw. Each row is an icon slot
//! and a name, then Modified, Type and Size; Modified and Size stay empty
//! until the core has read that entry's details. A thumbnail, once it
//! arrives, takes the icon's place. In pick mode a click selects an entry
//! and the pick bar runs along the bottom (see `pick`).
//!
//! The window sets the key context its actions are bound in and handles
//! each action it takes; buttons and the mouse's side buttons send actions
//! rather than doing the work (see `commands`). Every element a test
//! clicks carries a debug selector: `back`, `forward`, `up`, `place-<n>`
//! and `entry-<n>`, counted from 0. Each line's Modified cell carries one
//! too, `modified-header` and `modified-<n>`, so a test can check the
//! columns line up.
//!
//! Every row spans the list's full width: `uniform_list` lays each row out
//! on its own, and a row sized to its content would push its columns along
//! by the width of its name.

use std::sync::Arc;

use gpui::{
    Action, AnyElement, ClickEvent, Context, Focusable as _, Hsla, InteractiveElement as _,
    IntoElement, MouseButton, MouseDownEvent, NavigationDirection, ParentElement as _, Render,
    RenderImage, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, img,
    prelude::FluentBuilder as _, rgb, svg, uniform_list,
};
use nova_files_api::{EntryKind, Place, PlaceKind};

use crate::assets::Icon;
use crate::commands::{self, GoBack, GoForward, GoUp};
use crate::theme::{color, opacity, radius, size};
use crate::window::MainWindow;

impl Render for MainWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.set_scale_factor(window.scale_factor(), window, cx);
        // Keys always land somewhere: in a main window on the window
        // itself, which a click inside takes focus back to; in a picker in
        // its name box, whose keys pass the window's on.
        let main = self.pick.is_none();
        if window.focused(cx).is_none() {
            let focus = self.pick.as_ref().map_or_else(
                || self.focus.clone(),
                |pick| pick.name.read(cx).focus_handle(cx),
            );
            window.focus(&focus, cx);
        }
        div()
            .key_context(commands::KEY_CONTEXT)
            .when(main, |root| root.track_focus(&self.focus))
            .on_action(cx.listener(|this, _: &GoUp, _, _| this.go_up()))
            .on_action(cx.listener(|this, _: &GoBack, _, _| this.go_back()))
            .on_action(cx.listener(|this, _: &GoForward, _, _| this.go_forward()))
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(|this, _: &MouseDownEvent, _, _| this.go_back()),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                cx.listener(|this, _: &MouseDownEvent, _, _| this.go_forward()),
            )
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(color::SURFACE))
            .text_color(rgb(color::TEXT))
            .text_size(size::TEXT)
            .child(self.toolbar())
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.places_column(cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .p(size::PAD)
                            .child(column_header())
                            .child(self.entries(cx)),
                    ),
            )
            .children(self.pick.as_ref().map(|pick| Self::pick_bar(pick, cx)))
    }
}

impl MainWindow {
    fn toolbar(&self) -> impl IntoElement {
        let back = button(
            "back",
            Icon::Back,
            None,
            self.back_target().is_some(),
            &GoBack,
        );
        let forward = button(
            "forward",
            Icon::Forward,
            None,
            self.forward_target().is_some(),
            &GoForward,
        );
        let up = button(
            "up",
            Icon::Up,
            Some(commands::UP.label),
            self.parent().is_some(),
            &GoUp,
        );
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
            .child(back)
            .child(forward)
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
            cx.processor(|this, range: std::ops::Range<usize>, window, cx| {
                this.rows_drawn(range.clone(), window, cx);
                let Some(listing) = &this.listing else {
                    return Vec::new();
                };
                let listing_id = listing.id;
                listing
                    .entries
                    .get(range.clone())
                    .unwrap_or_default()
                    .iter()
                    .zip(range.start..)
                    .map(|(entry, index)| {
                        let id = entry.id;
                        let text = this.columns(index).unwrap_or_default();
                        let name = SharedString::from(entry.name.to_string_lossy().into_owned());
                        let row = row(("entry", index), format!("entry-{index}")).child(cells(
                            div()
                                .flex()
                                .items_center()
                                .gap(size::GAP)
                                .child(entry_icon(entry.kind, this.thumbnail(index)))
                                .child(div().min_w_0().truncate().child(name)),
                            text.modified,
                            text.type_name,
                            text.size,
                            format!("modified-{index}"),
                        ));
                        let selected = this.pick_selects((listing_id, id));
                        row.when(selected, |row| row.bg(rgb(color::ROW_SELECTED)))
                            .when(this.pick_dims(index), |row| row.opacity(opacity::DIMMED))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, event, window, cx| {
                                this.entry_clicked((listing_id, id), event, window, cx);
                            }))
                            .into_any_element()
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.scroll)
        .flex_1()
    }
}

/// The labels above the entries, lined up with [`cells`].
fn column_header() -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(size::HEADER_HEIGHT)
        .px(size::PAD)
        .text_size(size::TEXT_SMALL)
        .text_color(rgb(color::TEXT_DIM))
        .border_b(size::DIVIDER)
        .border_color(rgb(color::DIVIDER))
        .child(cells(
            div().child(NAME_LABEL),
            MODIFIED_LABEL,
            TYPE_LABEL,
            SIZE_LABEL,
            MODIFIED_HEADER_SELECTOR.to_owned(),
        ))
}

/// One line of the columns: the name takes what the others leave. The
/// Modified cell carries `modified_selector`.
fn cells(
    name: impl IntoElement,
    modified: impl Into<SharedString>,
    type_name: impl Into<SharedString>,
    size_text: impl Into<SharedString>,
    modified_selector: String,
) -> impl IntoElement {
    let column = |width, text: SharedString| {
        div()
            .flex_none()
            .w(width)
            .truncate()
            .text_color(rgb(color::TEXT_DIM))
            .child(text)
    };
    div()
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .gap(size::GAP)
        .child(div().flex_1().min_w_0().child(name))
        .child(column(size::MODIFIED_WIDTH, modified.into()).debug_selector(|| modified_selector))
        .child(column(size::TYPE_WIDTH, type_name.into()))
        .child(column(size::SIZE_WIDTH, size_text.into()).text_right())
}

/// A toolbar button with selector `name`: an icon, and `label` beside it
/// when the icon alone isn't enough. When `enabled`, a click sends
/// `action`; otherwise the button is dimmed and does nothing.
fn button(
    name: &'static str,
    glyph: Icon,
    label: Option<&'static str>,
    enabled: bool,
    action: &dyn Action,
) -> impl IntoElement {
    let tone = if enabled {
        color::TEXT
    } else {
        color::TEXT_DIM
    };
    let action = action.boxed_clone();
    div()
        .id(name)
        .debug_selector(|| name.to_owned())
        .flex()
        .items_center()
        .gap(size::BUTTON_GAP)
        .h(size::ROW_HEIGHT)
        .px(size::PAD)
        .rounded(radius::BUTTON)
        .border(size::DIVIDER)
        .border_color(rgb(color::DIVIDER))
        .text_color(rgb(tone))
        .child(icon(glyph, size::BUTTON_ICON, rgb(tone).into()))
        .children(label)
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(|button| button.bg(rgb(color::ROW_HIGHLIGHT)))
                .on_click(move |_: &ClickEvent, window, cx| {
                    window.dispatch_action(action.boxed_clone(), cx);
                })
        })
}

/// One row of either list, as wide as the list: icon slot and label,
/// highlighted under the pointer.
fn row(id: (&'static str, usize), selector: String) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .debug_selector(|| selector)
        .w_full()
        .flex()
        .flex_none()
        .items_center()
        .gap(size::GAP)
        .h(size::ROW_HEIGHT)
        .px(size::PAD)
        .rounded(radius::ROW)
        .hover(|row| row.bg(rgb(color::ROW_HIGHLIGHT)))
}

/// The icon slot of an entry row: its thumbnail if it has one, otherwise
/// its kind's icon, centred in the thumbnail's square so nothing shifts
/// when the thumbnail arrives.
fn entry_icon(kind: EntryKind, thumbnail: Option<&Arc<RenderImage>>) -> impl IntoElement {
    let slot = div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(size::THUMBNAIL);
    match (thumbnail, kind) {
        (Some(image), _) => slot.child(
            img(Arc::clone(image))
                .size(size::THUMBNAIL)
                .rounded(radius::THUMBNAIL),
        ),
        (None, EntryKind::Folder) => slot.child(icon(
            Icon::Folder,
            size::ROW_ICON,
            rgb(color::FOLDER).into(),
        )),
        (None, EntryKind::File) => {
            slot.child(icon(Icon::File, size::ROW_ICON, rgb(color::FILE).into()))
        }
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
const NAME_LABEL: &str = "Name";
const MODIFIED_LABEL: &str = "Modified";
const TYPE_LABEL: &str = "Type";
const SIZE_LABEL: &str = "Size";
const MODIFIED_HEADER_SELECTOR: &str = "modified-header";

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
