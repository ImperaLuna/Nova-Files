use gpui::{Bounds, TextRun, black, font, point, px, size};

use super::{CARET_WIDTH, Colors, Mark, runs, scroll, span};

fn run(len: usize) -> TextRun {
    TextRun {
        len,
        font: font("Inter"),
        color: black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    }
}

/// Each run's length and whether it is underlined.
fn shape(runs: &[TextRun]) -> Vec<(usize, bool)> {
    runs.iter()
        .map(|run| (run.len, run.underline.is_some()))
        .collect()
}

#[test]
fn plain_text_is_one_run() {
    assert_eq!(shape(&runs(run(5), None)), [(5, false)]);
}

#[test]
fn composed_text_is_underlined_between_the_rest() {
    let runs = runs(run(9), Some(2..5));
    assert_eq!(shape(&runs), [(2, false), (3, true), (4, false)]);
    assert_eq!(runs[1].underline.and_then(|line| line.color), Some(black()));
}

#[test]
fn composed_text_at_an_end_leaves_no_empty_run() {
    assert_eq!(shape(&runs(run(4), Some(0..4))), [(4, true)]);
    assert_eq!(shape(&runs(run(4), Some(1..4))), [(1, false), (3, true)]);
}

#[test]
fn a_caret_in_view_scrolls_nothing() {
    assert_eq!(scroll(px(0.0), px(40.0), px(80.0), px(100.0)), px(0.0));
}

#[test]
fn a_caret_past_the_right_edge_scrolls_it_into_view() {
    let width = px(100.0);
    let scrolled = scroll(px(0.0), px(150.0), px(200.0), width);
    assert_eq!(scrolled, px(150.0) - (width - CARET_WIDTH));
    // It stays put while the caret is in view.
    assert_eq!(scroll(scrolled, px(120.0), px(200.0), width), scrolled);
}

#[test]
fn a_caret_left_of_the_view_scrolls_back_to_it() {
    assert_eq!(scroll(px(90.0), px(30.0), px(400.0), px(100.0)), px(30.0));
}

#[test]
fn shorter_text_scrolls_back_to_fit() {
    let width = px(100.0);
    // The line now ends 20 pixels past the field, the caret before that.
    let line = width - CARET_WIDTH + px(20.0);
    assert_eq!(scroll(px(90.0), px(60.0), line, width), px(20.0));
    assert_eq!(scroll(px(90.0), px(60.0), px(50.0), width), px(0.0));
}

#[test]
fn a_field_too_narrow_for_the_caret_scrolls_to_it() {
    assert_eq!(scroll(px(0.0), px(30.0), px(30.0), px(1.0)), px(30.0));
}

#[test]
fn a_span_sits_along_the_scrolled_line() {
    let bounds = Bounds::new(point(px(10.0), px(5.0)), size(px(100.0), px(20.0)));
    let span = span(bounds, px(4.0), px(6.0), px(16.0));
    assert_eq!(
        span,
        Bounds::from_corners(point(px(12.0), px(5.0)), point(px(22.0), px(25.0)))
    );
}

#[test]
fn the_caret_shows_only_while_focused() {
    let bounds = Bounds::new(point(px(1.0), px(2.0)), size(px(3.0), px(4.0)));
    assert!(Mark::Caret(bounds).quad(false, Colors::default()).is_none());
    assert_eq!(
        Mark::Caret(bounds)
            .quad(true, Colors::default())
            .map(|quad| quad.bounds),
        Some(bounds)
    );
    for focused in [false, true] {
        let quad = Mark::Selection(bounds).quad(focused, Colors::default());
        assert_eq!(quad.map(|quad| quad.bounds), Some(bounds));
    }
}

#[test]
fn an_empty_selection_is_the_caret() {
    let bounds = Bounds::new(point(px(10.0), px(0.0)), size(px(100.0), px(20.0)));
    assert_eq!(
        Mark::new(bounds, px(2.0), px(5.0), px(5.0)),
        Mark::Caret(span(bounds, px(2.0), px(5.0), px(5.0) + CARET_WIDTH))
    );
    assert_eq!(
        Mark::new(bounds, px(2.0), px(5.0), px(9.0)),
        Mark::Selection(span(bounds, px(2.0), px(5.0), px(9.0)))
    );
}
