use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Grapheme,
    Word,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextEdit {
    text: String,
    selection: Range<usize>,
    reversed: bool,
    marked: Option<Range<usize>>,
}

impl TextEdit {
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

    pub const fn caret(&self) -> usize {
        if self.reversed {
            self.selection.start
        } else {
            self.selection.end
        }
    }

    const fn anchor(&self) -> usize {
        if self.reversed {
            self.selection.end
        } else {
            self.selection.start
        }
    }

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

    pub fn move_left(&mut self, step: Step, extend: bool) {
        if extend {
            self.select_to(self.before(self.caret(), step));
        } else if self.selection.is_empty() || step == Step::Word {
            self.move_to(self.before(self.selection.start, step));
        } else {
            self.move_to(self.selection.start);
        }
    }

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

    pub fn delete_backward(&mut self, step: Step) -> bool {
        if self.selection.is_empty() {
            let caret = self.caret();
            self.selection = self.before(caret, step)..caret;
        }
        self.delete_selection()
    }

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

    pub fn replace_selection(&mut self, new: &str) -> bool {
        self.replace(Some(self.selection.clone()), new)
    }

    pub fn replace(&mut self, range: Option<Range<usize>>, new: &str) -> bool {
        let range = self.target(range);
        let changed = !(range.is_empty() && new.is_empty());
        self.text.replace_range(range.clone(), new);
        self.move_to(range.start + new.len());
        changed
    }

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

    pub const fn unmark(&mut self) {
        self.marked = None;
    }

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

    pub fn select_to(&mut self, offset: usize) {
        let anchor = self.anchor();
        self.select_range(anchor..offset);
    }

    fn before(&self, offset: usize, step: Step) -> usize {
        let mut graphemes = self.text[..offset].grapheme_indices(true).rev();
        match step {
            Step::Grapheme => graphemes.next().map_or(0, |(index, _)| index),
            Step::Word => {
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

    fn after(&self, offset: usize, step: Step) -> usize {
        let mut graphemes = self.text[offset..]
            .grapheme_indices(true)
            .map(|(index, grapheme)| (offset + index, grapheme));
        match step {
            Step::Grapheme => graphemes
                .next()
                .map_or(offset, |(index, grapheme)| index + grapheme.len()),
            Step::Word => {
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

    fn floor(&self, offset: usize) -> usize {
        self.text.floor_char_boundary(offset)
    }

    pub fn offset_to_utf16(&self, offset: usize) -> usize {
        self.text[..self.floor(offset)].encode_utf16().count()
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Word,
    Space,
    Other,
}

impl Kind {
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
