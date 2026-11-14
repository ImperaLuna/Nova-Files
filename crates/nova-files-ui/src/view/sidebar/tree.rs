//! The "This computer" section: the folder tree under Home (see `tree`),
//! under Places and scrolling with them. Each row is indented by its
//! depth, with a hairline down each level it sits in. The arrow opens and
//! closes a folder in place; a click elsewhere on the row goes there. The
//! folder you're in has its name strong and its icon orange. Rows carry
//! the debug selectors `tree-<n>` and their arrows `tree-toggle-<n>`,
//! counted from 0 down the tree.

use std::f32::consts::FRAC_PI_2;
use std::path::PathBuf;

use gpui::{
    ClickEvent, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    StatefulInteractiveElement as _, Styled as _, Transformation, div, prelude::FluentBuilder as _,
    radians, svg,
};

use super::{icon, section_head};
use crate::assets::Icon;
use crate::places;
use crate::theme::{Colors, radius, size, text};
use crate::tree::{Toggle, TreeRow};
use crate::window::MainWindow;

const LABEL: &str = "This computer";

impl MainWindow {
    /// The tree section, with its head.
    pub(super) fn tree_section(&self, cx: &Context<Self>) -> Div {
        let colors = Colors::of(cx);
        let rows = self
            .tree
            .rows()
            .into_iter()
            .enumerate()
            .map(|(index, row)| self.tree_row(index, row, &colors, cx));
        div()
            .flex()
            .flex_col()
            .child(section_head(LABEL, None, &colors))
            .child(div().flex().flex_col().gap(size::PLACE_GAP).children(rows))
    }

    fn tree_row(
        &self,
        index: usize,
        row: TreeRow,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let TreeRow {
            path,
            name,
            depth,
            toggle,
        } = row;
        let here = self.folder() == Some(path.as_path());
        let (glyph, name) = if depth == 0 {
            (Icon::Home, places::HOME_LABEL.into())
        } else {
            let glyph = places::standard_icon_at(self.places(), &path).unwrap_or(Icon::Folder);
            (glyph, name)
        };
        let target = path.clone();
        let tint = if here { colors.here } else { colors.text_dim };
        let guides = (1..=depth).map(|level| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(guide_left(level))
                .w(size::LINE)
                .bg(colors.line)
        });
        div()
            .id(("tree", index))
            .debug_selector(move || format!("tree-{index}"))
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .w_full()
            .h(size::TREE_ROW_HEIGHT)
            .gap(size::TREE_ROW_GAP)
            .pl(indent(depth) + size::TREE_PAD_LEFT)
            .pr(size::TREE_PAD_RIGHT)
            .rounded(radius::ROW)
            .cursor_pointer()
            .whitespace_nowrap()
            .hover(|row| row.bg(colors.hover))
            .when(here, |row| {
                row.text_color(colors.text).font_weight(text::STRONG_WEIGHT)
            })
            .on_click(cx.listener(move |this, _: &ClickEvent, _, _| this.open(&target)))
            .children(guides)
            .child(tree_toggle(index, path, toggle, colors, cx))
            .child(icon(glyph, size::TREE_ICON, tint))
            .child(div().flex_1().min_w_0().truncate().child(name))
    }
}

/// The arrow before a folder's icon: a button while it can open or close,
/// an empty slot once it has no subfolders, so names line up.
fn tree_toggle(
    index: usize,
    path: PathBuf,
    toggle: Toggle,
    colors: &Colors,
    cx: &Context<MainWindow>,
) -> impl IntoElement {
    let slot = div()
        .id(("tree-toggle", index))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(size::TREE_TOGGLE)
        .rounded(radius::SMALL);
    if toggle == Toggle::None {
        return slot;
    }
    let angle = if toggle == Toggle::Open {
        FRAC_PI_2
    } else {
        0.0
    };
    slot.debug_selector(move || format!("tree-toggle-{index}"))
        .cursor_pointer()
        .hover(|button| button.bg(colors.press))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            cx.stop_propagation();
            this.toggle_tree(&path, cx);
        }))
        .child(
            svg()
                .path(Icon::Chevron.path())
                .size(size::TREE_TOGGLE_ICON)
                .text_color(colors.text_faint)
                .with_transformation(Transformation::rotate(radians(angle))),
        )
}

/// How far in a row at `depth` starts.
fn indent(depth: usize) -> Pixels {
    // A tree deeper than this is far off the sidebar's edge anyway.
    let depth = u16::try_from(depth).unwrap_or(u16::MAX);
    size::TREE_INDENT * f32::from(depth)
}

/// Where the hairline for `level` runs: at the start of that level's
/// indent, a little before the rows inside it.
fn guide_left(level: usize) -> Pixels {
    indent(level) - size::TREE_GUIDE_INSET
}
