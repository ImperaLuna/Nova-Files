use super::{Step, TextEdit};

/// `text` with `selection` selected, the caret at its end.
fn at(text: &str, selection: std::ops::Range<usize>) -> TextEdit {
    let mut edit = TextEdit::new(text);
    edit.select_range(selection);
    edit
}

#[test]
fn new_text_has_the_caret_at_its_end() {
    let edit = TextEdit::new("notes.txt");
    assert_eq!(edit.text(), "notes.txt");
    assert_eq!(edit.selection(), 9..9);
    assert_eq!(edit.caret(), 9);
    assert_eq!(edit.marked(), None);
}

#[test]
fn typing_replaces_the_selection() {
    let mut edit = at("notes.txt", 0..5);
    edit.replace(None, "plan");
    assert_eq!(edit.text(), "plan.txt");
    assert_eq!(edit.selection(), 4..4);
}

#[test]
fn replacing_a_range_puts_the_caret_after_it() {
    let mut edit = TextEdit::new("abc");
    edit.replace(Some(1..2), "XY");
    assert_eq!(edit.text(), "aXYc");
    assert_eq!(edit.selection(), 3..3);
}

#[expect(
    clippy::reversed_empty_ranges,
    reason = "a backwards range is one of the cases"
)]
#[test]
fn a_range_past_the_end_is_kept_inside_the_text() {
    let mut edit = TextEdit::new("abc");
    edit.replace(Some(2..99), "!");
    assert_eq!(edit.text(), "ab!");
    let mut backwards = TextEdit::new("abc");
    backwards.replace(Some(2..1), "!");
    assert_eq!(backwards.text(), "ab!c");
}

#[test]
fn grapheme_moves_step_over_a_whole_character() {
    // "e" and a combining accent read as one character.
    let mut edit = TextEdit::new("cafe\u{301}!");
    edit.move_left(Step::Grapheme, false);
    assert_eq!(edit.caret(), 6);
    edit.move_left(Step::Grapheme, false);
    assert_eq!(edit.caret(), 3);
    edit.move_right(Step::Grapheme, false);
    assert_eq!(edit.caret(), 6);
}

#[test]
fn moves_stop_at_the_ends() {
    let mut edit = TextEdit::new("ab");
    edit.move_right(Step::Grapheme, false);
    assert_eq!(edit.caret(), 2);
    edit.move_home(false);
    edit.move_left(Step::Grapheme, false);
    edit.move_left(Step::Word, false);
    assert_eq!(edit.caret(), 0);
}

#[test]
fn a_plain_move_collapses_the_selection_to_its_side() {
    let mut left = at("abcdef", 2..4);
    left.move_left(Step::Grapheme, false);
    assert_eq!(left.selection(), 2..2);
    let mut right = at("abcdef", 2..4);
    right.move_right(Step::Grapheme, false);
    assert_eq!(right.selection(), 4..4);
}

#[test]
fn word_moves_split_a_file_name_at_dots_and_dashes() {
    let mut edit = TextEdit::new("my report-2026.final.pdf");
    let mut stops = Vec::new();
    for _ in 0..7 {
        edit.move_left(Step::Word, false);
        stops.push(edit.caret());
    }
    assert_eq!(stops, [21, 15, 10, 3, 0, 0, 0]);
    let mut stops = Vec::new();
    for _ in 0..7 {
        edit.move_right(Step::Word, false);
        stops.push(edit.caret());
    }
    assert_eq!(stops, [2, 9, 14, 20, 24, 24, 24]);
}

#[test]
fn a_word_move_goes_from_the_selection_edge() {
    let mut left = at("one two three", 5..9);
    left.move_left(Step::Word, false);
    assert_eq!(left.selection(), 4..4);
    let mut right = at("one two three", 4..6);
    right.move_right(Step::Word, false);
    assert_eq!(right.selection(), 7..7);
}

#[test]
fn shift_moves_grow_and_shrink_the_selection_from_its_anchor() {
    let mut edit = at("abcdef", 2..2);
    edit.move_right(Step::Grapheme, true);
    edit.move_right(Step::Grapheme, true);
    assert_eq!(edit.selection(), 2..4);
    assert_eq!(edit.selected_text(), "cd");
    edit.move_left(Step::Grapheme, true);
    edit.move_left(Step::Grapheme, true);
    edit.move_left(Step::Grapheme, true);
    assert_eq!(edit.selection(), 1..2);
    assert_eq!(edit.caret(), 1);
    edit.move_right(Step::Word, true);
    assert_eq!(edit.selection(), 2..6);
    assert_eq!(edit.caret(), 6);
}

#[test]
fn shift_home_and_end_select_to_the_ends() {
    let mut edit = at("abcdef", 3..3);
    edit.move_home(true);
    assert_eq!(edit.selection(), 0..3);
    assert_eq!(edit.caret(), 0);
    edit.move_end(true);
    assert_eq!(edit.selection(), 3..6);
    edit.move_home(false);
    assert_eq!(edit.selection(), 0..0);
    edit.move_end(false);
    assert_eq!(edit.selection(), 6..6);
}

#[expect(
    clippy::reversed_empty_ranges,
    reason = "a backwards range is one of the cases"
)]
#[test]
fn select_all_and_select_range() {
    let mut edit = TextEdit::new("notes.txt");
    edit.select_all();
    assert_eq!(edit.selection(), 0..9);
    edit.select_range(5..0);
    assert_eq!(edit.selection(), 0..5);
    assert_eq!(edit.caret(), 0);
    // Inside a character, a range moves back to its start.
    let mut accented = TextEdit::new("é!");
    accented.select_range(1..3);
    assert_eq!(accented.selection(), 0..3);
}

#[test]
fn backspace_and_delete_take_one_character_or_the_selection() {
    let mut edit = at("abcdef", 3..3);
    assert!(edit.delete_backward(Step::Grapheme));
    assert_eq!((edit.text(), edit.caret()), ("abdef", 2));
    assert!(edit.delete_forward(Step::Grapheme));
    assert_eq!((edit.text(), edit.caret()), ("abef", 2));
    edit.select_range(1..3);
    assert!(edit.delete_forward(Step::Word));
    assert_eq!((edit.text(), edit.caret()), ("af", 1));
}

#[test]
fn deleting_at_an_end_changes_nothing() {
    let mut start = at("ab", 0..0);
    assert!(!start.delete_backward(Step::Grapheme));
    assert!(!start.delete_backward(Step::Word));
    let mut end = TextEdit::new("ab");
    assert!(!end.delete_forward(Step::Grapheme));
    assert!(!end.delete_forward(Step::Word));
    assert_eq!(end.text(), "ab");
}

#[test]
fn word_deletes_take_a_word_and_the_separators_before_it() {
    let mut edit = TextEdit::new("report-2026.pdf");
    assert!(edit.delete_backward(Step::Word));
    assert_eq!(edit.text(), "report-2026.");
    assert!(edit.delete_backward(Step::Word));
    assert_eq!(edit.text(), "report-");
    edit.move_home(false);
    assert!(edit.delete_forward(Step::Word));
    assert_eq!(edit.text(), "-");
}

#[test]
fn double_click_selects_the_run_under_it() {
    let mut edit = TextEdit::new("my report-2026.pdf");
    edit.select_word_at(5);
    assert_eq!(edit.selected_text(), "report");
    edit.select_word_at(2);
    assert_eq!(edit.selected_text(), " ");
    edit.select_word_at(9);
    assert_eq!(edit.selected_text(), "-");
    // At the very end, the word before.
    edit.select_word_at(18);
    assert_eq!(edit.selected_text(), "pdf");
    let mut empty = TextEdit::new("");
    empty.select_word_at(0);
    assert_eq!(empty.selection(), 0..0);
}

#[test]
fn an_input_method_composes_then_commits() {
    let mut edit = TextEdit::new("a");
    edit.replace_and_mark(None, "に", None);
    assert_eq!(edit.text(), "aに");
    assert_eq!(edit.marked(), Some(1..4));
    assert_eq!(edit.selection(), 4..4);
    // The next composing step replaces what is marked.
    edit.replace_and_mark(None, "にほ", Some(0..3));
    assert_eq!(edit.text(), "aにほ");
    assert_eq!(edit.marked(), Some(1..7));
    assert_eq!(edit.selection(), 1..4);
    // The commit replaces the marked text too.
    edit.replace(None, "日本");
    assert_eq!(edit.text(), "a日本");
    assert_eq!(edit.marked(), None);
    assert_eq!(edit.selection(), 7..7);
}

#[test]
fn composing_nothing_marks_nothing() {
    let mut edit = TextEdit::new("ab");
    edit.replace_and_mark(None, "x", None);
    edit.replace_and_mark(None, "", None);
    assert_eq!(edit.text(), "ab");
    assert_eq!(edit.marked(), None);
    edit.replace_and_mark(None, "x", None);
    edit.unmark();
    assert_eq!((edit.text(), edit.marked()), ("abx", None));
}

#[test]
fn utf16_offsets_count_as_input_methods_do() {
    // "é" is two bytes and one unit; the emoji is four bytes and two units.
    let edit = TextEdit::new("é😀a");
    assert_eq!(edit.offset_to_utf16(0), 0);
    assert_eq!(edit.offset_to_utf16(2), 1);
    assert_eq!(edit.offset_to_utf16(6), 3);
    assert_eq!(edit.offset_to_utf16(7), 4);
    assert_eq!(edit.offset_from_utf16(1), 2);
    assert_eq!(edit.offset_from_utf16(3), 6);
    assert_eq!(edit.offset_from_utf16(4), 7);
    assert_eq!(edit.offset_from_utf16(99), 7);
    assert_eq!(edit.range_to_utf16(&(2..6)), 1..3);
    assert_eq!(edit.range_from_utf16(&(1..3)), 2..6);
}

#[test]
fn keys_while_composing_act_on_the_caret_not_the_composed_text() {
    let mut edit = TextEdit::new("a");
    edit.replace_and_mark(None, "にほ", None);
    assert!(edit.delete_backward(Step::Grapheme));
    assert_eq!(edit.text(), "aに");
    assert_eq!(edit.marked(), None);
}

#[test]
fn moving_the_caret_ends_composing() {
    let mut edit = TextEdit::new("a");
    edit.replace_and_mark(None, "に", None);
    edit.move_home(false);
    assert_eq!(edit.marked(), None);
    edit.replace(None, "x");
    assert_eq!(edit.text(), "xaに");
}

#[test]
fn a_word_keeps_its_accents() {
    // A decomposed "é": "e" and a combining accent.
    let mut edit = TextEdit::new("cafe\u{301}.txt");
    edit.move_home(false);
    edit.move_right(Step::Word, false);
    assert_eq!(edit.caret(), 6);
    edit.move_left(Step::Word, false);
    assert_eq!(edit.caret(), 0);
    edit.select_word_at(1);
    assert_eq!(edit.selected_text(), "cafe\u{301}");
}

#[test]
fn replacing_nothing_with_nothing_is_no_change() {
    let mut edit = TextEdit::new("ab");
    assert!(!edit.replace_selection(""));
    assert!(edit.replace_selection("c"));
    assert_eq!(edit.text(), "abc");
}

#[test]
fn a_word_delete_takes_the_last_word_to_the_end() {
    let mut edit = at("ab cd", 2..2);
    assert!(edit.delete_forward(Step::Word));
    assert_eq!((edit.text(), edit.caret()), ("ab", 2));
}
