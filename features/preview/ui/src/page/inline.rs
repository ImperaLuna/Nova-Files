use std::ops::Range;

use gpui::{
    FontStyle, HighlightStyle, SharedString, StrikethroughStyle, StyledText, UnderlineStyle,
};
use nova_files_preview_api::markdown::{Inline, Look, Span};
use nova_files_shell::theme::{Colors, size, text};

pub fn styled(inline: &Inline, colors: &Colors) -> StyledText {
    let highlights: Vec<(Range<usize>, HighlightStyle)> = ranges(&inline.spans)
        .filter_map(|(range, span)| Some((range, highlight(span, colors)?)))
        .collect();
    StyledText::new(SharedString::from(inline.text.to_string())).with_highlights(highlights)
}

fn ranges(spans: &[Span]) -> impl Iterator<Item = (Range<usize>, &Span)> {
    spans.iter().scan(0, |at: &mut usize, span| {
        let start = *at;
        *at += usize::try_from(span.bytes).unwrap_or(0);
        Some((start..*at, span))
    })
}

fn highlight(span: &Span, colors: &Colors) -> Option<HighlightStyle> {
    let look = span.look;
    let mut style = HighlightStyle::default();
    if look.has(Look::STRONG) {
        style.font_weight = Some(text::STRONG_WEIGHT);
    }
    if look.has(Look::EMPHASIS) {
        style.font_style = Some(FontStyle::Italic);
    }
    if look.has(Look::STRUCK) {
        style.strikethrough = Some(StrikethroughStyle {
            thickness: size::LINE,
            color: Some(colors.text_dim.into()),
        });
    }
    if look.has(Look::CODE) {
        style.color = Some(colors.code.quoted.into());
        style.background_color = Some(colors.raised.into());
    }
    if look.has(Look::FORMULA) {
        style.color = Some(colors.code.number.into());
    }
    if look.has(Look::NOTE) {
        style.color = Some(colors.text_dim.into());
    }
    if span.link.is_some() {
        style.color = Some(colors.code.function.into());
        style.underline = Some(UnderlineStyle {
            thickness: size::LINE,
            color: Some(colors.code.function.into()),
            wavy: false,
        });
    }
    (style != HighlightStyle::default()).then_some(style)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(bytes: u32, look: Look, link: Option<&str>) -> Span {
        Span {
            bytes,
            look,
            link: link.map(Into::into),
        }
    }

    #[test]
    fn ranges_follow_each_other() {
        let spans = [span(2, Look::PLAIN, None), span(3, Look::STRONG, None)];
        let ranges: Vec<_> = ranges(&spans).map(|(range, _)| range).collect();
        assert_eq!(ranges, [0..2, 2..5]);
    }

    #[test]
    fn plain_words_need_no_highlight_and_links_are_underlined() {
        let colors = Colors::default();
        assert!(highlight(&span(1, Look::PLAIN, None), &colors).is_none());
        let linked = highlight(&span(1, Look::PLAIN, Some("a.md")), &colors).unwrap();
        assert!(linked.underline.is_some());
        let both = highlight(&span(1, Look::STRONG.with(Look::EMPHASIS), None), &colors).unwrap();
        assert_eq!(both.font_weight, Some(text::STRONG_WEIGHT));
        assert_eq!(both.font_style, Some(FontStyle::Italic));
    }
}
