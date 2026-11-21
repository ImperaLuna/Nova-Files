use std::rc::Rc;
use std::sync::Arc;

use gpui::{App, Context, Font, Pixels, SharedString, TextRun, TruncateFrom, Window, font};
use nova_files_api::{Entry, FileIcon};
use nova_files_shell::kind::built_in;

use super::MainWindow;
use super::dragging::Drag;
use crate::pile::{Face, Label, Pile, PileDrawing};
use crate::theme::{Colors, pile, text};
use crate::view::entry_kind;

const TRUNCATED: &str = "…";

impl MainWindow {
    pub(super) fn draw_pile(&self, drag: &Rc<Drag>, window: &Window, cx: &Context<Self>) {
        let colors = Colors::of(cx);
        let entries = self.selected_entries();
        let label = match entries.as_slice() {
            [only] => Label::Name(only.name.to_string_lossy().into_owned()),
            many => Label::Count(many.len().to_string()),
        };
        let faces = entries
            .iter()
            .take(pile::MOST)
            .map(|entry| self.face(entry, &colors, cx))
            .collect();
        let (label, label_width) = fitted(label, window, cx);
        let shown = Pile {
            faces,
            label,
            label_width,
            colors,
        };
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a window's scale is a small positive number"
        )]
        let scale = window.scale_factor().ceil() as u32;
        let drawing = PileDrawing {
            pile: Arc::new(shown),
            renderer: cx.svg_renderer(),
            scale,
        };
        drag.drawing.set(drawing.clone()).ok();
        let drawn = cx
            .background_executor()
            .spawn(async move { drawing.draw() });
        let drag = Rc::clone(drag);
        cx.spawn(async move |_, _| {
            if let Some(picture) = drawn.await {
                drag.picture.set(picture).ok();
            }
        })
        .detach();
    }

    fn face(&self, entry: &Entry, colors: &Colors, cx: &App) -> Face {
        if let Some(picture) = self.parts.row_picture(entry, cx) {
            return Face::Thumbnail(picture);
        }
        let read = usize::try_from(entry.id.0)
            .ok()
            .and_then(|index| self.details.get(&index));
        let kind = match read.map(|read| &read.icon) {
            Some(FileIcon::Theme { icon, .. }) => return Face::Theme(Arc::clone(icon)),
            Some(FileIcon::BuiltIn(kind)) => *kind,
            None => entry_kind(entry.kind),
        };
        let (glyph, tint) = built_in(kind, colors);
        Face::Glyph { glyph, tint }
    }
}

fn fitted(label: Label, window: &Window, cx: &App) -> (Label, Pixels) {
    let style = font(text::FAMILY);
    let style = Font {
        weight: text::BODY_WEIGHT,
        ..style
    };
    let run = |len| TextRun {
        len,
        font: style.clone(),
        color: Colors::of(cx).text.into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let label = match label {
        Label::Name(name) => {
            let runs = [run(name.len())];
            let (shown, _) = cx
                .text_system()
                .line_wrapper(style.clone(), text::BODY)
                .truncate_line(
                    SharedString::from(name),
                    pile::NAME_WIDTH,
                    TRUNCATED,
                    &runs,
                    TruncateFrom::Middle,
                );
            Label::Name(shown.to_string())
        }
        count @ Label::Count(_) => count,
    };
    let words = match &label {
        Label::Name(words) | Label::Count(words) => words.clone(),
    };
    let width = window
        .text_system()
        .layout_line(&words, text::BODY, &[run(words.len())], None)
        .width;
    (label, width)
}
