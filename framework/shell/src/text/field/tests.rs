use super::super::edit::TextEdit;
use super::{DOT, dot_index, dots, offset_of_dot, word_left, word_right};

#[test]
fn hidden_text_shows_one_dot_a_character() {
    assert_eq!(dots("pässwörd"), DOT.to_string().repeat(8));
    assert_eq!(dots(""), "");
}

#[test]
fn offsets_map_to_dots_and_back() {
    let text = "aé€b";
    let dot = DOT.len_utf8();
    for (offset, index) in [(0, 0), (1, dot), (3, 2 * dot), (6, 3 * dot), (7, 4 * dot)] {
        assert_eq!(dot_index(text, offset), index, "{offset}");
        assert_eq!(offset_of_dot(text, index), offset, "{index}");
    }
    assert_eq!(offset_of_dot(text, 9 * dot), text.len());
}

#[test]
fn word_steps_in_hidden_text_go_to_the_ends() {
    let mut edit = TextEdit::new("my secret pw");
    word_left(&mut edit, true, false);
    assert_eq!(edit.selection(), 0..0);
    word_right(&mut edit, true, false);
    assert_eq!(edit.selection(), 12..12);
    word_left(&mut edit, false, false);
    assert_eq!(edit.selection(), 10..10);
}
