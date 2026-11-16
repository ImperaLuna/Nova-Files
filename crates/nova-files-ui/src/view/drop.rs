//! Drop targets: a folder row, the list of the folder on screen, and in
//! the sidebar and path bar the places, tree folders, drives, crumbs and
//! the trash. While dragged files would drop on one, it is filled and
//! edged in orange; a target where they can't drop (see `dropping`) stays
//! as it is and takes nothing. Files dragged out of a list and files from
//! another app drop alike.
//!
//! Each target keeps a clear edge of [`size::DROP_EDGE`], so the orange
//! edge shifts nothing; a target's padding gives that width back.

use std::any::Any;

use gpui::{
    Context, ExternalPaths, InteractiveElement, Pixels, StyleRefinement, Styled, Window,
    transparent_black,
};

use crate::dropping::{DropAt, Sources};
use crate::theme::{Colors, size};
use crate::window::{DraggedRow, MainWindow};

/// The padding that, with the clear drop edge, makes `pad`.
pub(super) fn inside_edge(pad: Pixels) -> Pixels {
    pad - size::DROP_EDGE
}

impl MainWindow {
    /// Makes `target` take files dropped at `at`, lit while they would.
    pub(super) fn drop_target<E: InteractiveElement + Styled>(
        &self,
        target: E,
        at: DropAt,
        cx: &Context<Self>,
    ) -> E {
        let colors = Colors::of(cx);
        let lit =
            move |style: StyleRefinement| style.bg(colors.drop).border_color(colors.drop_edge);
        self.takes_drops(target, at, cx)
            .border(size::DROP_EDGE)
            .border_color(transparent_black())
            .drag_over::<DraggedRow>(move |style, _, _, _| lit(style))
            .drag_over::<ExternalPaths>(move |style, _, _, _| lit(style))
    }

    /// Makes `target` take files dropped at `at`, without lighting up:
    /// for the list of the folder on screen, whose rows light up.
    pub(super) fn takes_drops<E: InteractiveElement>(
        &self,
        target: E,
        at: DropAt,
        cx: &Context<Self>,
    ) -> E {
        let rule = self.drop_rule();
        let (asked, dropped, theirs) = (at.clone(), at.clone(), at);
        target
            .can_drop(move |dragged: &dyn Any, _, _| {
                if let Some(row) = dragged.downcast_ref::<DraggedRow>() {
                    return row.sources().is_some_and(|sources| rule(&asked, sources));
                }
                // Another app's files, known only once they arrive.
                dragged
                    .downcast_ref::<ExternalPaths>()
                    .is_some_and(|paths| rule(&asked, &Sources::new(paths.paths().to_vec())))
            })
            .on_drop(
                cx.listener(move |this, row: &DraggedRow, window: &mut Window, _| {
                    if let Some(sources) = row.sources() {
                        this.drop_files(dropped.clone(), sources, window.modifiers());
                    }
                }),
            )
            .on_drop(
                cx.listener(move |this, paths: &ExternalPaths, window: &mut Window, _| {
                    let sources = Sources::new(paths.paths().to_vec());
                    this.drop_files(theirs.clone(), &sources, window.modifiers());
                }),
            )
    }
}
