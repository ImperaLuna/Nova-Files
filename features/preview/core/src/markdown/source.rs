use std::ops::Range;

use nova_files_preview_api::{Mark, Run};
use pulldown_cmark::{Event, Tag};

use crate::code::Runs;

pub struct Source {
    marks: Vec<Mark>,
}

impl Source {
    pub fn new(length: usize) -> Self {
        Self {
            marks: vec![Mark::Plain; length],
        }
    }

    pub fn paint(&mut self, event: &Event<'_>, range: Range<usize>) {
        let Some(mark) = mark(event) else {
            return;
        };
        if let Some(marks) = self.marks.get_mut(range) {
            marks.fill(mark);
        }
    }

    pub fn into_runs(self) -> Vec<Run> {
        let mut runs = Runs::default();
        for piece in self.marks.chunk_by(|one, next| one == next) {
            if let Some(mark) = piece.first() {
                runs.push(*mark, piece.len());
            }
        }
        runs.into_runs()
    }
}

const fn mark(event: &Event<'_>) -> Option<Mark> {
    Some(match event {
        Event::Start(Tag::Heading { .. }) | Event::TaskListMarker(_) => Mark::Keyword,
        Event::Start(Tag::CodeBlock(_)) | Event::Code(_) => Mark::Quoted,
        Event::Start(Tag::Link { .. } | Tag::Image { .. }) => Mark::Function,
        Event::InlineMath(_) | Event::DisplayMath(_) => Mark::Number,
        Event::Start(Tag::HtmlBlock) | Event::Html(_) | Event::InlineHtml(_) => Mark::Tag,
        Event::FootnoteReference(_) => Mark::Attribute,
        Event::Start(Tag::BlockQuote(_) | Tag::MetadataBlock(_)) | Event::Rule => Mark::Comment,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::read;

    fn marked(text: &str) -> Vec<(String, Mark)> {
        let (_, runs) = read(text, || false).unwrap();
        let mut at = 0;
        runs.iter()
            .map(|run| {
                let end = at + run.bytes as usize;
                let piece = text[at..end].to_owned();
                at = end;
                (piece, run.mark)
            })
            .collect()
    }

    #[test]
    fn the_source_is_marked_from_end_to_end() {
        let text = "# Title\n\nSee `code` and [a link](x.md).\n";
        let (_, runs) = read(text, || false).unwrap();
        let covered: u32 = runs.iter().map(|run| run.bytes).sum();
        assert_eq!(covered as usize, text.len());
        let marked = marked(text);
        assert!(marked.contains(&("# Title\n".to_owned(), Mark::Keyword)));
        assert!(marked.contains(&("`code`".to_owned(), Mark::Quoted)));
        assert!(marked.contains(&("[a link](x.md)".to_owned(), Mark::Function)));
    }

    #[test]
    fn an_inner_piece_keeps_its_own_mark() {
        let marked = marked("> quoted `code`\n");
        assert!(marked.contains(&("`code`".to_owned(), Mark::Quoted)));
        assert_eq!(marked.first().map(|(_, mark)| *mark), Some(Mark::Comment));
    }
}
