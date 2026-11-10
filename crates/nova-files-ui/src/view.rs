//! Draws the main window: a toolbar with Back, Forward, Up and the
//! folder's path, the places column, and the folder's entries under a
//! column header.
//!
//! The entries are a `uniform_list`, which builds only the rows on screen,
//! so a folder of any size costs the same to draw. Each row is an icon slot
//! and a name, then Modified, Type and Size; Modified and Size stay empty
//! until the core has read that entry's details. A thumbnail, once it
//! arrives, takes the icon's place. Selected rows are tinted, and the
//! row the keyboard is on has a ring (see `selecting`). In pick mode the
//! pick bar runs along the bottom (see `pick`).
//!
//! The window sets the key context its actions are bound in and handles
//! each action it takes; buttons and the mouse's side buttons send actions
//! rather than doing the work (see `commands`). A press on the entries
//! that isn't on a name can drag a box to select (see `band`). Every
//! element a test clicks carries a debug selector: `back`, `forward`, `up`,
//! `place-<n>`, `entry-<n>` and its name `name-<n>`, counted from 0, and
//! the box `band`. Each
//! line's Modified cell carries one too, `modified-header` and
//! `modified-<n>`, so a test can check the columns line up.
//!
//! Every row spans the list's full width: `uniform_list` lays each row out
//! on its own, and a row sized to its content would push its columns along
//! by the width of its name.

use std::sync::Arc;

use gpui::{
    Action, AnyElement, ClickEvent, Context, Div, Focusable as _, Hsla, InteractiveElement as _,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    NavigationDirection, ParentElement as _, Render, RenderImage, ScrollWheelEvent, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, img, prelude::FluentBuilder as _,
    svg, uniform_list,
};
use nova_files_api::{EntryKind, Place, PlaceKind};

use crate::assets::Icon;
use crate::commands::{
    self, ClearSelection, CursorDown, CursorFirst, CursorLast, CursorPageDown, CursorPageUp,
    CursorUp, GoBack, GoForward, GoUp, OpenSelected, SelectAll, SelectDown, SelectFirst,
    SelectLast, SelectPageDown, SelectPageUp, SelectUp, ToggleSelected,
};
use crate::selection::Move;
use crate::theme::{Colors, opacity, radius, size};
use crate::window::MainWindow;

impl Render for MainWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.set_scale_factor(window.scale_factor(), window, cx);
        let colors = Colors::of(cx);
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
            .map(|root| Self::selection_keys(root, cx))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if this.jump_typed(event, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                this.band_moved(event, cx);
            }))
            .on_scroll_wheel(cx.listener(|this, _: &ScrollWheelEvent, _, cx| {
                this.band_scrolled(cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _, cx| this.band_released(event, cx)),
            )
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
            .bg(colors.surface)
            .text_color(colors.text)
            .text_size(size::TEXT)
            .child(self.toolbar(colors))
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
                            .child(column_header(colors))
                            .child(self.entries_area(cx)),
                    ),
            )
            .children(self.pick.as_ref().map(|pick| Self::pick_bar(pick, cx)))
    }
}

impl MainWindow {
    /// The selection's actions on the window's root (see `selecting`).
    fn selection_keys(root: Div, cx: &Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &OpenSelected, _, _| this.open_selected()))
            .on_action(cx.listener(|this, _: &CursorUp, _, cx| {
                this.move_cursor(Move::Up, false, cx);
            }))
            .on_action(cx.listener(|this, _: &CursorDown, _, cx| {
                this.move_cursor(Move::Down, false, cx);
            }))
            .on_action(cx.listener(|this, _: &CursorPageUp, _, cx| {
                this.move_cursor(Move::PageUp, false, cx);
            }))
            .on_action(cx.listener(|this, _: &CursorPageDown, _, cx| {
                this.move_cursor(Move::PageDown, false, cx);
            }))
            .on_action(cx.listener(|this, _: &CursorFirst, _, cx| {
                this.move_cursor(Move::First, false, cx);
            }))
            .on_action(cx.listener(|this, _: &CursorLast, _, cx| {
                this.move_cursor(Move::Last, false, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectUp, _, cx| {
                this.move_cursor(Move::Up, true, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectDown, _, cx| {
                this.move_cursor(Move::Down, true, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectPageUp, _, cx| {
                this.move_cursor(Move::PageUp, true, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectPageDown, _, cx| {
                this.move_cursor(Move::PageDown, true, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectFirst, _, cx| {
                this.move_cursor(Move::First, true, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectLast, _, cx| {
                this.move_cursor(Move::Last, true, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| this.select_all(cx)))
            .on_action(cx.listener(|this, _: &ToggleSelected, _, cx| this.toggle_cursor(cx)))
            .on_action(cx.listener(|this, _: &ClearSelection, _, cx| this.clear_selection(cx)))
    }

    fn toolbar(&self, colors: Colors) -> impl IntoElement {
        let back = button(
            "back",
            Icon::Back,
            None,
            self.back_target().is_some().then_some(&GoBack),
            colors,
        );
        let forward = button(
            "forward",
            Icon::Forward,
            None,
            self.forward_target().is_some().then_some(&GoForward),
            colors,
        );
        let up = button(
            "up",
            Icon::Up,
            Some(commands::UP.label),
            self.parent().is_some().then_some(&GoUp),
            colors,
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
            .bg(colors.surface_sunk)
            .border_b(size::DIVIDER)
            .border_color(colors.divider)
            .child(back)
            .child(forward)
            .child(up)
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(size::TEXT_SMALL)
                    .text_color(colors.text_dim)
                    .child(path),
            )
    }

    fn places_column(&self, cx: &Context<Self>) -> impl IntoElement {
        let colors = Colors::of(cx);
        let open = self.folder();
        let rows: Vec<AnyElement> = self
            .places()
            .iter()
            .enumerate()
            .map(|(index, place)| {
                let is_open = open == Some(place.path.as_path());
                let target = place.path.clone();
                let tint = if is_open {
                    colors.open_icon
                } else {
                    colors.icon
                };
                row(("place", index), format!("place-{index}"), colors)
                    .when(is_open, |row| row.bg(colors.open))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, _| {
                        this.open(&target);
                    }))
                    .child(icon(Icon::Folder, size::ROW_ICON, tint.into()))
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
            .bg(colors.surface_sunk)
            .border_r(size::DIVIDER)
            .border_color(colors.divider)
            .children(rows)
    }

    /// The entries, where a press that isn't on a name can drag a box
    /// (see `band`).
    fn entries_area(&self, cx: &Context<Self>) -> impl IntoElement {
        let colors = Colors::of(cx);
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, _| {
                    let on_name = std::mem::take(&mut this.pressed_name);
                    this.band_pressed(event, on_name);
                }),
            )
            .child(self.entries(cx))
            .children(self.band_box().map(|band| {
                div()
                    .debug_selector(|| "band".to_owned())
                    .absolute()
                    .left(band.origin.x)
                    .top(band.origin.y)
                    .w(band.size.width)
                    .h(band.size.height)
                    .rounded(radius::BAND)
                    .bg(Hsla::from(colors.selected).opacity(opacity::BAND))
                    .border(size::DIVIDER)
                    .border_color(colors.cursor)
            }))
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
                let colors = Colors::of(cx);
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
                        let row = row(("entry", index), format!("entry-{index}"), colors)
                            .text_color(colors.text_dim)
                            .child(cells(
                                div()
                                    .text_color(colors.text)
                                    .flex()
                                    .items_center()
                                    .gap(size::GAP)
                                    .child(entry_icon(entry.kind, this.thumbnail(index), colors))
                                    .child(entry_name(name, index, cx)),
                                text.modified,
                                text.type_name,
                                text.size,
                                format!("modified-{index}"),
                            ));
                        let selected = this.selects((listing_id, id));
                        let cursor = this.cursor() == Some(index);
                        row.when(selected, |row| row.bg(colors.selected))
                            .when(cursor, |row| row.relative().child(cursor_ring(colors)))
                            .when(this.pick_dims(index), |row| row.opacity(opacity::DIMMED))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, event, window, cx| {
                                this.row_clicked((listing_id, id), event, window, cx);
                            }))
                            .into_any_element()
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full()
    }
}

/// An entry's name. A press on it is left for dragging files, so it
/// marks itself for `band`.
fn entry_name(name: SharedString, index: usize, cx: &Context<MainWindow>) -> impl IntoElement {
    div()
        .debug_selector(move || format!("name-{index}"))
        .min_w_0()
        .truncate()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, _| this.pressed_name = true),
        )
        .child(name)
}

/// The labels above the entries, lined up with [`cells`].
fn column_header(colors: Colors) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(size::HEADER_HEIGHT)
        .px(size::PAD)
        .text_size(size::TEXT_SMALL)
        .text_color(colors.text_dim)
        .border_b(size::DIVIDER)
        .border_color(colors.divider)
        .child(cells(
            div().child(NAME_LABEL),
            MODIFIED_LABEL,
            TYPE_LABEL,
            SIZE_LABEL,
            MODIFIED_HEADER_SELECTOR.to_owned(),
        ))
}

/// One line of the columns: the name takes what the others leave. The
/// Modified cell carries `modified_selector`. The caller colors the line,
/// and the name apart from it.
fn cells(
    name: impl IntoElement,
    modified: impl Into<SharedString>,
    type_name: impl Into<SharedString>,
    size_text: impl Into<SharedString>,
    modified_selector: String,
) -> impl IntoElement {
    let column = |width, text: SharedString| div().flex_none().w(width).truncate().child(text);
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
/// when the icon alone isn't enough. A click sends `action`; without one
/// the button is dimmed and does nothing.
fn button(
    name: &'static str,
    glyph: Icon,
    label: Option<&'static str>,
    action: Option<&dyn Action>,
    colors: Colors,
) -> impl IntoElement {
    let tone = if action.is_some() {
        colors.text
    } else {
        colors.text_dim
    };
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
        .border_color(colors.divider)
        .text_color(tone)
        .child(icon(glyph, size::BUTTON_ICON, tone.into()))
        .children(label)
        .when_some(action.map(Action::boxed_clone), |button, action| {
            button
                .cursor_pointer()
                .hover(|button| button.bg(colors.hover))
                .on_click(move |_: &ClickEvent, window, cx| {
                    window.dispatch_action(action.boxed_clone(), cx);
                })
        })
}

/// One row of either list, as wide as the list: icon slot and label,
/// highlighted under the pointer.
fn row(id: (&'static str, usize), selector: String, colors: Colors) -> gpui::Stateful<Div> {
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
        .hover(|row| row.bg(colors.hover))
}

/// The ring around the row the keyboard is on, laid over the row so it
/// shifts nothing.
fn cursor_ring(colors: Colors) -> impl IntoElement {
    div()
        .absolute()
        .inset_0()
        .rounded(radius::ROW)
        .border(size::DIVIDER)
        .border_color(colors.cursor)
}

/// The icon slot of an entry row: its thumbnail if it has one, otherwise
/// its kind's icon, centred in the thumbnail's square so nothing shifts
/// when the thumbnail arrives.
fn entry_icon(
    kind: EntryKind,
    thumbnail: Option<&Arc<RenderImage>>,
    colors: Colors,
) -> impl IntoElement {
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
        (None, EntryKind::Folder) => {
            slot.child(icon(Icon::Folder, size::ROW_ICON, colors.folder.into()))
        }
        (None, EntryKind::File) => slot.child(icon(Icon::File, size::ROW_ICON, colors.icon.into())),
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
        PlaceKind::Desktop
        | PlaceKind::Documents
        | PlaceKind::Downloads
        | PlaceKind::Music
        | PlaceKind::Pictures
        | PlaceKind::Videos => place
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
