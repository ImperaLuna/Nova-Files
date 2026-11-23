use gpui::{
    App, AppContext as _, Context, ElementId, Font, InteractiveElement as _, InteractiveText,
    IntoElement, ParentElement as _, Pixels, Render, SharedString, Styled as _, StyledText,
    TextRun, Window, div, font,
};

use crate::theme::{Colors, radius, size, text};

const BOTH_SIDES: f32 = 2.0;

pub fn full_name_on_hover(
    id: impl Into<ElementId>,
    shown: StyledText,
    full: SharedString,
) -> InteractiveText {
    let laid_out = shown.layout().clone();
    InteractiveText::new(id, shown).tooltip(move |_, window, cx| {
        let cut = !cx.has_active_drag() && laid_out.text() != full.as_ref();
        cut.then(|| {
            let width = name_width(&full, window, cx);
            cx.new(|_| FullName {
                name: full.clone(),
                width,
            })
            .into()
        })
    })
}

fn name_width(name: &SharedString, window: &Window, cx: &App) -> Pixels {
    let run = TextRun {
        len: name.len(),
        font: Font {
            weight: text::BODY_WEIGHT,
            ..font(text::FAMILY)
        },
        color: Colors::of(cx).text.into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window
        .text_system()
        .shape_line(name.clone(), text::BODY, &[run], None);
    (line.width + (size::DRAG_CHIP_PAD + size::LINE) * BOTH_SIDES).min(size::FULL_NAME_WIDTH)
}

struct FullName {
    name: SharedString,
    width: Pixels,
}

impl Render for FullName {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::of(cx);
        div()
            .debug_selector(|| "full-name".to_owned())
            .w(self.width)
            .px(size::DRAG_CHIP_PAD)
            .py(size::DRAG_CHIP_PAD_Y)
            .rounded(radius::CONTROL)
            .bg(colors.lifted)
            .border(size::LINE)
            .border_color(colors.line)
            .font_family(text::FAMILY)
            .font_weight(text::BODY_WEIGHT)
            .text_size(text::BODY)
            .text_color(colors.text)
            .child(self.name.clone())
    }
}
