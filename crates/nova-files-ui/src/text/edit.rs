//! The text and selection of a one-line text field, apart from drawing and
//! keys, so every editing rule is a plain function of its state.
//!
//! Offsets are bytes into the text and always sit on a character boundary.
//! The caret moves by grapheme (what reads as one character) or by word. A
//! word is a run of letters, digits and `_`, so the dots and dashes of a
//! file name split it.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation as _;

/// How far a caret move or delete goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// One grapheme.
    Grapheme,
    /// To the edge of a word.
    Word,
}

/// A field's text, its selection and any text an input method is still
/// composing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextEdit {
    text: String,
    /// `start <= end`; empty when it is just the caret.
    selection: Range<usize>,
    /// The caret is at the selection's start rather than its end.
    reversed: bool,
    /// What an input method is composing, shown underlined.
    marked: Option<Range<usize>>,
}

impl TextEdit {
    /// `text` with the caret at its end.
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            selection: text.len()..text.len(),
            reversed: false,
            marked: None,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn selection(&self) -> Range<usize> {
        self.selection.clone()
    }

    pub fn selected_text(&self) -> &str {
        &self.text[self.selection.clone()]
    }

    pub fn marked(&self) -> Option<Range<usize>> {
        self.marked.clone()
    }

    /// Where the caret is: the end of the selection it moved last.
    pub const fn caret(&self) -> usize {
        if self.reversed {
            self.selection.start
        } else {
            self.selection.end
        }
    }

    /// The end of the selection that stays put while it grows.
    const fn anchor(&self) -> usize {
        if self.reversed {
            self.selection.end
        } else {
            self.selection.start
        }
    }

    /// Selects `range`, kept inside the text and on character boundaries.
    /// Moving the caret ends any composing: the next text goes at the caret.
    pub fn select_range(&mut self, range: Range<usize>) {
        let start = self.floor(range.start);
        let end = self.floor(range.end);
        self.selection = start.min(end)..start.max(end);
        self.reversed = end < start;
        self.marked = None;
    }

    pub fn select_all(&mut self) {
        self.select_range(0..self.text.len());
    }

    /// Selects the word, space or punctuation run at `offset`, as a
    /// double-click does.
    pub fn select_word_at(&mut self, offset: usize) {
        let offset = self.floor(offset);
        let Some(kind) = self.text[offset..]
            .graphemes(true)
            .next()
            .or_else(|| self.text[..offset].graphemes(true).next_back())
            .map(Kind::of)
        else {
            return;
        };
        let start = self.text[..offset]
            .grapheme_indices(true)
            .rev()
            .take_while(|&(_, g)| Kind::of(g) == kind)
            .last()
            .map_or(offset, |(index, _)| index);
        let end = self.text[offset..]
            .grapheme_indices(true)
            .find(|&(_, g)| Kind::of(g) != kind)
            .map_or(self.text.len(), |(index, _)| offset + index);
        self.select_range(start..end);
    }

    /// Moves the caret left by `step`, or grows the selection when
    /// `extend`. Without `extend`, a selection collapses to its start.
    pub fn move_left(&mut self, step: Step, extend: bool) {
        if extend {
            self.select_to(self.before(self.caret(), step));
        } else if self.selection.is_empty() || step == Step::Word {
            self.move_to(self.before(self.selection.start, step));
        } else {
            self.move_to(self.selection.start);
        }
    }

    /// Moves the caret right by `step`, or grows the selection when
    /// `extend`. Without `extend`, a selection collapses to its end.
    pub fn move_right(&mut self, step: Step, extend: bool) {
        if extend {
            self.select_to(self.after(self.caret(), step));
        } else if self.selection.is_empty() || step == Step::Word {
            self.move_to(self.after(self.selection.end, step));
        } else {
            self.move_to(self.selection.end);
        }
    }

    pub fn move_home(&mut self, extend: bool) {
        if extend {
            self.select_to(0);
        } else {
            self.move_to(0);
        }
    }

    pub fn move_end(&mut self, extend: bool) {
        if extend {
            self.select_to(self.text.len());
        } else {
            self.move_to(self.text.len());
        }
    }

    /// Deletes the selection, or `step` before the caret. Whether the text
    /// changed.
    pub fn delete_backward(&mut self, step: Step) -> bool {
        if self.selection.is_empty() {
            let caret = self.caret();
            self.selection = self.before(caret, step)..caret;
        }
        self.delete_selection()
    }

    /// Deletes the selection, or `step` after the caret. Whether the text
    /// changed.
    pub fn delete_forward(&mut self, step: Step) -> bool {
        if self.selection.is_empty() {
            let caret = self.caret();
            self.selection = caret..self.after(caret, step);
        }
        self.delete_selection()
    }

    fn delete_selection(&mut self) -> bool {
        self.replace_selection("")
    }

    /// Replaces the selection with `new`, as a key, a cut or a paste does,
    /// and puts the caret after it. Whether the text changed.
    pub fn replace_selection(&mut self, new: &str) -> bool {
        self.replace(Some(self.selection.clone()), new)
    }

    /// Replaces `range`, or else the text being composed, or else the
    /// selection, with `new`, and puts the caret after it. This is how an
    /// input method's text arrives, typed or composed. Whether the text
    /// changed.
    pub fn replace(&mut self, range: Option<Range<usize>>, new: &str) -> bool {
        let range = self.target(range);
        let changed = !(range.is_empty() && new.is_empty());
        self.text.replace_range(range.clone(), new);
        self.move_to(range.start + new.len());
        changed
    }

    /// Like [`Self::replace`], but `new` stays marked as text an input
    /// method is still composing, with `selected` (within `new`) selected.
    pub fn replace_and_mark(
        &mut self,
        range: Option<Range<usize>>,
        new: &str,
        selected: Option<Range<usize>>,
    ) {
        let range = self.target(range);
        self.text.replace_range(range.clone(), new);
        let start = range.start;
        let selected = selected.unwrap_or(new.len()..new.len());
        self.select_range(start + selected.start..start + selected.end);
        self.marked = (!new.is_empty()).then(|| start..start + new.len());
    }

    /// The input method is done composing; the text stays as it is.
    pub const fn unmark(&mut self) {
        self.marked = None;
    }

    /// The range a replacement changes: `range` kept inside the text, or
    /// what is being composed, or the selection.
    fn target(&self, range: Option<Range<usize>>) -> Range<usize> {
        range
            .map(|range| {
                let start = self.floor(range.start);
                start..self.floor(range.end).max(start)
            })
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection.clone())
    }

    fn move_to(&mut self, offset: usize) {
        self.select_range(offset..offset);
    }

    /// Moves the caret to `offset`, keeping the anchor, as a shift-click
    /// or a drag does.
    pub fn select_to(&mut self, offset: usize) {
        let anchor = self.anchor();
        self.select_range(anchor..offset);
    }

    /// The boundary `step` before `offset`.
    fn before(&self, offset: usize, step: Step) -> usize {
        let mut graphemes = self.text[..offset].grapheme_indices(true).rev();
        match step {
            Step::Grapheme => graphemes.next().map_or(0, |(index, _)| index),
            Step::Word => {
                // Past any separators, then to the start of the word.
                let mut start = offset;
                let mut in_word = false;
                for (index, grapheme) in graphemes {
                    let word = Kind::of(grapheme) == Kind::Word;
                    if in_word && !word {
                        break;
                    }
                    in_word |= word;
                    if in_word {
                        start = index;
                    }
                }
                if in_word { start } else { 0 }
            }
        }
    }

    /// The boundary `step` after `offset`.
    fn after(&self, offset: usize, step: Step) -> usize {
        let mut graphemes = self.text[offset..]
            .grapheme_indices(true)
            .map(|(index, grapheme)| (offset + index, grapheme));
        match step {
            Step::Grapheme => graphemes
                .next()
                .map_or(offset, |(index, grapheme)| index + grapheme.len()),
            Step::Word => {
                // Past any separators, then to the end of the word.
                let mut in_word = false;
                graphemes
                    .find(|&(_, grapheme)| {
                        let word = Kind::of(grapheme) == Kind::Word;
                        let done = in_word && !word;
                        in_word |= word;
                        done
                    })
                    .map_or(self.text.len(), |(index, _)| index)
            }
        }
    }

    /// `offset` kept inside the text, moved back to a character boundary.
    fn floor(&self, offset: usize) -> usize {
        self.text.floor_char_boundary(offset)
    }

    /// `offset` in UTF-16 code units, as input methods count.
    pub fn offset_to_utf16(&self, offset: usize) -> usize {
        self.text[..self.floor(offset)].encode_utf16().count()
    }

    /// The byte offset of `utf16` UTF-16 code units in.
    pub fn offset_from_utf16(&self, utf16: usize) -> usize {
        offset_from_utf16(&self.text, utf16)
    }

    pub fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    pub fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }
}

/// The byte offset of `utf16` UTF-16 code units into `text`.
pub fn offset_from_utf16(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (index, c) in text.char_indices() {
        if units >= utf16 {
            return index;
        }
        units += c.len_utf16();
    }
    text.len()
}

/// What a character counts as for word moves and double-clicks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Word,
    Space,
    Other,
}

impl Kind {
    /// What `grapheme` counts as, by its first character, so an accent
    /// that follows a letter stays with it.
    fn of(grapheme: &str) -> Self {
        let c = grapheme.chars().next().unwrap_or(' ');
        if c.is_alphanumeric() || c == '_' {
            Self::Word
        } else if c.is_whitespace() {
            Self::Space
        } else {
            Self::Other
        }
    }
}

#[cfg(test)]
mod tests;
