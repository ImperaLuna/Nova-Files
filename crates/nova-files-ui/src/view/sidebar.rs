//! The sidebar, on the window to the left of the bar and the sheet, in
//! three parts as on the design page: the Open list fixed at the top (the
//! tabs fill it; for now it is an empty slot, `open`), then Places, which
//! scroll, then the drives fixed at the very bottom (`drives`), which
//! scroll on their own past a few. Each
//! section is drawn in a file of its own; this file lays them out and
//! draws the section heads and the rows they share.
//!
//! Places and the folder tree ("This computer") scroll together.
//!
//! A narrow window folds the sidebar into one row above the bar: the
//! section heads and the tree go, and the places sit side by side and
//! scroll across.
//!
//! A row is a place you can go: its icon, its name and, on the right, a
//! number when it has one. The row you're in has its name strong and its
//! icon orange; pressed-in rows are left for the Open list, as on the
//! design page.

mod drives;
mod places;
mod tree;

use gpui::{
    AnyElement, Context, Div, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    Rgba, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, div,
    prelude::FluentBuilder as _,
};

use super::row::icon;
use crate::assets::Icon;
use crate::theme::{Colors, radius, size, text};
use crate::window::MainWindow;

impl MainWindow {
    /// The sidebar; folded into one row across the window when `narrow`.
    pub(super) fn sidebar(&self, narrow: bool, cx: &Context<Self>) -> impl IntoElement {
        let open = div()
            .debug_selector(|| "open".to_owned())
            .flex()
            .flex_none();
        let drives = div()
            .debug_selector(|| "drives".to_owned())
            .flex()
            .flex_none()
            .child(self.drives_section(narrow, cx));
        let places = self.places_section(narrow, cx);
        let sidebar = div()
            .id("sidebar")
            .debug_selector(|| "sidebar".to_owned())
            .flex()
            .flex_none();
        if narrow {
            return sidebar
                .w_full()
                .items_center()
                .gap(size::PLACE_GAP)
                .pr(size::GAP)
                .pb(size::GAP)
                .overflow_x_scroll()
                .child(open)
                .child(places)
                .child(drives);
        }
        sidebar
            .flex_col()
            .w(size::SIDEBAR_WIDTH)
            .child(open.flex_col())
            .child(
                div()
                    .id("sidebar-scroll")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .gap(size::PLACE_GAP)
                    .pb(size::GAP)
                    .overflow_y_scroll()
                    .child(places)
                    .when(!narrow, |scroll| scroll.child(self.tree_section(cx))),
            )
            .child(drives.flex_col().max_h(size::DRIVES_MAX_HEIGHT))
    }
}

/// A section's head: its name, and a button on the right when it has one.
fn section_head(label: &'static str, button: Option<AnyElement>, colors: &Colors) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_between()
        .pt(size::SECTION_TOP)
        .pr(size::SECTION_RIGHT)
        .pb(size::SECTION_BOTTOM)
        .pl(size::PLACE_PAD)
        .text_size(text::LABEL)
        .font_weight(text::LABEL_WEIGHT)
        .text_color(colors.text_dim)
        .child(label)
        .children(button)
}

/// The button on a section's head: `glyph` on a small square, carrying
/// the debug selector `selector`. Ready for its click.
fn section_button(selector: &'static str, glyph: Icon, colors: &Colors) -> Stateful<Div> {
    div()
        .id(selector)
        .debug_selector(move || selector.to_owned())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(size::SECTION_BUTTON)
        .rounded(radius::SMALL)
        .cursor_pointer()
        .hover(|button| button.bg(colors.hover))
        .child(icon(glyph, size::SECTION_BUTTON_ICON, colors.text_dim))
}

/// What a sidebar row shows.
pub struct Row {
    pub id: ElementId,
    pub selector: String,
    pub glyph: Icon,
    pub name: SharedString,
    /// A number on the right, such as how much space is free.
    pub number: Option<(SharedString, Rgba)>,
    /// Whether you're in it.
    pub here: bool,
}

/// A sidebar row, ready for its click; compact when the sidebar is
/// `narrow`.
fn row(row: Row, narrow: bool, colors: &Colors) -> Stateful<Div> {
    let Row {
        id,
        selector,
        glyph,
        name,
        number,
        here,
    } = row;
    let tint = if here { colors.here } else { colors.text_dim };
    div()
        .id(id)
        .debug_selector(move || selector)
        .flex()
        .flex_none()
        .items_center()
        .h(size::PLACE_HEIGHT)
        .rounded(radius::ROW)
        .cursor_pointer()
        .whitespace_nowrap()
        .hover(|row| row.bg(colors.hover))
        .map(|row| {
            if narrow {
                row.gap(size::PLACE_NARROW_GAP).px(size::PLACE_NARROW_PAD)
            } else {
                row.w_full().gap(size::PLACE_PAD).px(size::PLACE_PAD)
            }
        })
        .when(here, |row| {
            row.text_color(colors.text).font_weight(text::STRONG_WEIGHT)
        })
        .child(icon(glyph, size::PLACE_ICON, tint))
        .child(div().flex_1().min_w_0().truncate().child(name))
        .children(number.filter(|_| !narrow).map(|(number, color)| {
            div()
                .flex_none()
                .text_size(text::LABEL)
                .font_weight(text::DETAIL_WEIGHT)
                .text_color(color)
                .child(number)
        }))
}
