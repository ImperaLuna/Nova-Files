mod drives;
mod open;
mod part_rows;
mod places;
mod tree;

use gpui::{
    AnyElement, ClickEvent, Context, Div, ElementId, InteractiveElement as _, IntoElement,
    ParentElement as _, Rgba, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _,
    StyledText, div, prelude::FluentBuilder as _, transparent_black,
};

use super::drop::inside_edge;
use super::full_name::full_name_on_hover;
use super::row::icon;
use crate::assets::Icon;
use crate::theme::{Colors, radius, size, text};
use crate::window::MainWindow;
use nova_files_shell::Resizable;

impl MainWindow {
    pub(super) fn sidebar(&self, narrow: bool, cx: &Context<Self>) -> impl IntoElement {
        let open = self
            .open_section(narrow, cx)
            .map(|open| open.debug_selector(|| "open".to_owned()));
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
                .overflow_x_scroll()
                .children(open)
                .child(places)
                .child(drives);
        }
        let drives = drives.flex_col();
        let (scrolled, kept) = if self.picker() {
            (Some(drives), None)
        } else {
            (None, Some(drives.max_h(size::DRIVES_MAX_HEIGHT)))
        };
        sidebar
            .flex_col()
            .flex_1()
            .min_h_0()
            .w(self.width_of(Resizable::Sidebar, cx))
            .children(open)
            .child(
                div()
                    .id("sidebar-scroll")
                    .debug_selector(|| "sidebar-scroll".to_owned())
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .gap(size::PLACE_GAP)
                    .pb(size::GAP)
                    .overflow_y_scroll()
                    .child(places)
                    .when(!narrow, |scroll| scroll.child(self.tree_section(cx)))
                    .children(scrolled),
            )
            .children(kept)
    }
}

fn section_head(
    label: &'static str,
    buttons: impl IntoIterator<Item = AnyElement>,
    colors: &Colors,
) -> Div {
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
        .child(div().flex().items_center().children(buttons))
}

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

pub struct Row {
    pub id: ElementId,
    pub selector: String,
    pub glyph: Icon,
    pub name: SharedString,
    pub number: Option<(SharedString, Rgba)>,
    pub here: bool,
    pub pressed: bool,
    pub lead: Option<AnyElement>,
}

pub(super) fn row(row: Row, narrow: bool, colors: &Colors) -> Stateful<Div> {
    let Row {
        id,
        selector,
        glyph,
        name,
        number,
        here,
        pressed,
        lead,
    } = row;
    let tint = if here { colors.here } else { colors.text_dim };
    let number_selector = format!("{selector}-number");
    let name_id = SharedString::from(format!("{selector}-name"));
    let here_selector = format!("{selector}-here");
    div()
        .id(id)
        .debug_selector(move || selector)
        .flex()
        .flex_none()
        .items_center()
        .h(size::PLACE_HEIGHT)
        .rounded(radius::ROW)
        .whitespace_nowrap()
        .map(|row| {
            if pressed {
                row.bg(colors.press)
            } else {
                row.hover(|row| row.bg(colors.hover))
            }
        })
        .border(size::DROP_EDGE)
        .border_color(transparent_black())
        .map(|row| {
            if narrow {
                row.gap(size::PLACE_NARROW_GAP)
                    .px(inside_edge(size::PLACE_NARROW_PAD))
            } else {
                row.w_full()
                    .gap(size::PLACE_PAD)
                    .px(inside_edge(size::PLACE_PAD))
            }
        })
        .when(here, |row| {
            row.text_color(colors.text).font_weight(text::STRONG_WEIGHT)
        })
        .children(lead)
        .child(
            div()
                .when(here, |glyph| glyph.debug_selector(move || here_selector))
                .child(icon(glyph, size::PLACE_ICON, tint)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(full_name_on_hover(
                    name_id,
                    StyledText::new(name.clone()),
                    name,
                )),
        )
        .children(number.map(|(number, color)| {
            div()
                .debug_selector(move || number_selector)
                .flex_none()
                .text_size(text::LABEL)
                .font_weight(text::DETAIL_WEIGHT)
                .text_color(color)
                .child(number)
        }))
}

pub(super) fn on_middle_click(
    element: Stateful<Div>,
    enabled: bool,
    cx: &Context<MainWindow>,
    act: impl Fn(&mut MainWindow, &mut Context<MainWindow>) + 'static,
) -> Stateful<Div> {
    element.when(enabled, |element| {
        element.on_aux_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
            if event.is_middle_click() {
                act(this, cx);
            }
        }))
    })
}
