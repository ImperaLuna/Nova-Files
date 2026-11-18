use std::any::Any;
use std::rc::Rc;

use gpui::{
    Context, DragMoveEvent, ExternalPaths, InteractiveElement, MouseMoveEvent, Pixels,
    StyleRefinement, Styled, Window, transparent_black,
};

use crate::dropping::Sources;
use crate::theme::{Colors, size};
use crate::window::{Aim, DraggedRow, DraggedTab, HoverOpens, MainWindow};

pub(super) fn inside_edge(pad: Pixels) -> Pixels {
    pad - size::DROP_EDGE
}

impl MainWindow {
    pub(super) fn drop_target<E: InteractiveElement + Styled>(
        &self,
        target: E,
        aim: Aim,
        cx: &Context<Self>,
    ) -> E {
        let colors = Colors::of(cx);
        let lit =
            move |style: StyleRefinement| style.bg(colors.drop).border_color(colors.drop_edge);
        self.takes_drops(target, aim, cx)
            .border(size::DROP_EDGE)
            .border_color(transparent_black())
            .drag_over::<DraggedRow>(move |style, _, _, _| lit(style))
            .drag_over::<ExternalPaths>(move |style, _, _, _| lit(style))
    }

    pub(super) fn opens_on_hover<E: InteractiveElement>(
        target: E,
        opens: HoverOpens,
        cx: &Context<Self>,
    ) -> E {
        target.on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, _, cx| {
            if cx.has_active_drag() {
                this.hovered_while_dragging(opens.clone());
            }
        }))
    }

    pub(super) fn takes_drops<E: InteractiveElement>(
        &self,
        target: E,
        aim: Aim,
        cx: &Context<Self>,
    ) -> E {
        let rule = Rc::new(self.drop_rule());
        let aims = Rc::clone(&rule);
        let (asked, dropped, theirs) = (aim.at.clone(), aim.at.clone(), aim.at.clone());
        target
            .on_drag_move(
                cx.listener(|this, moved: &DragMoveEvent<DraggedRow>, _, cx| {
                    this.drag_moved(moved.drag(cx).drag());
                }),
            )
            .on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, _, _| {
                if let Some(drag) = this.drag_over()
                    && drag.sources().is_some_and(|sources| aims(&aim.at, sources))
                {
                    drag.aim(aim.clone());
                }
            }))
            .can_drop(move |dragged: &dyn Any, _, _| {
                if dragged.is::<DraggedTab>() {
                    return true;
                }
                if let Some(row) = dragged.downcast_ref::<DraggedRow>() {
                    return row.sources().is_some_and(|sources| rule(&asked, sources));
                }
                dragged
                    .downcast_ref::<ExternalPaths>()
                    .is_some_and(|paths| rule(&asked, &Sources::new(paths.paths().to_vec())))
            })
            .on_drop(
                cx.listener(move |this, row: &DraggedRow, window: &mut Window, cx| {
                    if let Some(sources) = row.sources() {
                        this.drop_files(dropped.clone(), sources, window.modifiers(), cx);
                    }
                }),
            )
            .on_drop(cx.listener(
                move |this, paths: &ExternalPaths, window: &mut Window, cx| {
                    let sources = Sources::new(paths.paths().to_vec());
                    this.drop_files(theirs.clone(), &sources, window.modifiers(), cx);
                },
            ))
    }
}
