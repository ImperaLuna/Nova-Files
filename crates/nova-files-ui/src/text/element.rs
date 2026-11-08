//! Draws a [`TextField`]: its line, the selection or caret, and the
//! underline under text an input method is composing. It also hands the
//! window the field's input handler while the field has focus.

use std::ops::Range;

use gpui::{
    App, Bounds, ContentMask, Element, ElementId, ElementInputHandler, Entity, Focusable,
    GlobalElementId, InspectorElementId, IntoElement, LayoutId, PaintQuad, Pixels, ShapedLine,
    Style, TextAlign, TextRun, UnderlineStyle, Window, fill, point, px, relative, rgb,
};

use super::field::TextField;
use crate::theme::color;

/// The caret's width.
const CARET_WIDTH: Pixels = px(1.5);

pub struct TextElement {
    field: Entity<TextField>,
}

impl TextElement {
    pub const fn new(field: Entity<TextField>) -> Self {
        Self { field }
    }
}

pub struct Prepainted {
    line: ShapedLine,
    mark: Mark,
}

/// The selection, or the caret when nothing is selected.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Mark {
    Caret(Bounds<Pixels>),
    Selection(Bounds<Pixels>),
}

impl Mark {
    /// The selection from `start` to `end` along a line drawn in `bounds`
    /// and scrolled by `scroll`, or the caret when the two meet.
    fn new(bounds: Bounds<Pixels>, scroll: Pixels, start: Pixels, end: Pixels) -> Self {
        if start == end {
            Self::Caret(span(bounds, scroll, start, start + CARET_WIDTH))
        } else {
            Self::Selection(span(bounds, scroll, start, end))
        }
    }

    /// What to paint: the selection always, the caret only while the
    /// field has focus.
    fn quad(self, focused: bool) -> Option<PaintQuad> {
        match self {
            Self::Caret(caret) => focused.then(|| fill(caret, rgb(color::CARET))),
            Self::Selection(selected) => Some(fill(selected, rgb(color::SELECTION))),
        }
    }
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepainted;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let field = self.field.read(cx);
        let edit = &field.edit;
        let style = window.text_style();
        let (shown, text_color) = if edit.text().is_empty() {
            (field.placeholder.clone(), rgb(color::TEXT_DIM).into())
        } else {
            (edit.text().to_owned().into(), rgb(color::TEXT).into())
        };
        let run = TextRun {
            len: shown.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        // Composed text is never empty, so the placeholder has none.
        let runs = runs(run, edit.marked());
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(shown, font_size, &runs, None);

        let caret_x = line.x_for_index(edit.caret());
        let scroll = scroll(field.scroll, caret_x, line.width(), bounds.size.width);
        let selection = edit.selection();
        let mark = Mark::new(
            bounds,
            scroll,
            line.x_for_index(selection.start),
            line.x_for_index(selection.end),
        );
        self.field.update(cx, |field, _| field.scroll = scroll);
        Prepainted { line, mark }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut Self::RequestLayoutState,
        prepainted: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.field.read(cx).focus_handle(cx);
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.field.clone()),
            cx,
        );
        let focused = focus.is_focused(window);
        let scroll = self.field.read(cx).scroll;
        let line = prepainted.line.clone();
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            if let Some(quad) = prepainted.mark.quad(focused) {
                window.paint_quad(quad);
            }
            let origin = span(bounds, scroll, px(0.0), px(0.0)).origin;
            // Painting a shaped line only fails when the glyph atlas is
            // full; the next frame tries again.
            line.paint(
                origin,
                window.line_height(),
                TextAlign::Left,
                None,
                window,
                cx,
            )
            .ok();
        });
        self.field.update(cx, |field, _| {
            // The placeholder is no text to point into.
            field.layout = (!field.edit.text().is_empty()).then_some((line, bounds));
        });
    }
}

/// The runs that draw `run`'s text, with `marked` underlined.
fn runs(run: TextRun, marked: Option<Range<usize>>) -> Vec<TextRun> {
    let Some(marked) = marked else {
        return vec![run];
    };
    let underline = UnderlineStyle {
        color: Some(run.color),
        thickness: px(1.0),
        wavy: false,
    };
    [
        TextRun {
            len: marked.start,
            ..run.clone()
        },
        TextRun {
            len: marked.len(),
            underline: Some(underline),
            ..run.clone()
        },
        TextRun {
            len: run.len - marked.end,
            ..run
        },
    ]
    .into_iter()
    .filter(|run| run.len > 0)
    .collect()
}

/// How far a line `line_width` wide scrolls in a field `width` wide: from
/// `previous`, just enough to keep the caret at `caret_x` in view, and
/// never past the line's end, so shorter text scrolls back.
fn scroll(previous: Pixels, caret_x: Pixels, line_width: Pixels, width: Pixels) -> Pixels {
    let room = (width - CARET_WIDTH).max(px(0.0));
    previous
        .min(caret_x)
        .min((line_width - room).max(px(0.0)))
        .max(caret_x - room)
}

/// The box from `start` to `end` along a line drawn in `bounds` and
/// scrolled left by `scroll`.
pub fn span(bounds: Bounds<Pixels>, scroll: Pixels, start: Pixels, end: Pixels) -> Bounds<Pixels> {
    let left = bounds.left() - scroll;
    Bounds::from_corners(
        point(left + start, bounds.top()),
        point(left + end, bounds.bottom()),
    )
}

#[cfg(test)]
mod tests;
