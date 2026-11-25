use std::mem;
use std::sync::Arc;

use nova_files_preview_api::markdown::{Align, Block, FigureKind, Inline, Item, Look, Span};
use nova_files_preview_api::{PREVIEW_TEXT_BYTES, Run};
use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::code;
use source::Source;

mod html;
mod source;

const DEEPEST: usize = 12;
const DIAGRAM_LANGUAGE: &str = "mermaid";
const FORMULA_LANGUAGE: &str = "math";
const SOFT_BREAK: &str = " ";
const HARD_BREAK: &str = "\n";
const EVENTS_BETWEEN_CHECKS: usize = 512;
const FENCES_COLOURED: usize = 64;

pub fn read(text: &str, cancelled: impl Fn() -> bool) -> Option<(Vec<Block>, Vec<Run>)> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_MATH
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;
    let mut page = Page::default();
    let mut source = Source::new(text.len());
    let events = Parser::new_ext(text, options).into_offset_iter();
    for (index, (event, range)) in events.enumerate() {
        if index % EVENTS_BETWEEN_CHECKS == 0 && cancelled() {
            return None;
        }
        source.paint(&event, range);
        page.take(event);
    }
    Some((page.finish(), source.into_runs()))
}

enum Holder {
    Quote(Vec<Block>),
    List {
        first: Option<u64>,
        items: Vec<Item>,
    },
    Item {
        checked: Option<bool>,
        blocks: Vec<Block>,
    },
    Note {
        label: Arc<str>,
        blocks: Vec<Block>,
    },
    Flat,
}

#[derive(Clone, Copy)]
enum Words {
    Paragraph,
    Heading(u8),
    Cell,
}

#[derive(Default)]
struct Writing {
    text: String,
    spans: Vec<Span>,
}

impl Writing {
    fn push(&mut self, piece: &str, look: Look, link: Option<&Arc<str>>) {
        let Ok(bytes) = u32::try_from(piece.len()) else {
            return;
        };
        if bytes == 0 {
            return;
        }
        self.text.push_str(piece);
        match self.spans.last_mut() {
            Some(last) if last.look == look && last.link.as_ref() == link => last.bytes += bytes,
            _ => self.spans.push(Span {
                bytes,
                look,
                link: link.cloned(),
            }),
        }
    }

    fn take(&mut self) -> Inline {
        let Self { text, spans } = mem::take(self);
        Inline {
            text: text.into(),
            spans: spans.into(),
        }
    }
}

#[derive(Default)]
struct Table {
    aligns: Vec<Align>,
    head: Vec<Inline>,
    rows: Vec<Arc<[Inline]>>,
    row: Vec<Inline>,
    in_head: bool,
}

#[derive(Default)]
struct Page {
    root: Vec<Block>,
    holders: Vec<Holder>,
    words: Option<Words>,
    writing: Writing,
    looks: Vec<Look>,
    links: Vec<Arc<str>>,
    code: Option<(Option<String>, String)>,
    picture: Option<(Arc<str>, String)>,
    html: Option<String>,
    tables: Vec<Table>,
    metadata: bool,
    colouring: Colouring,
}

#[derive(Default)]
struct Colouring {
    fences: usize,
    bytes: usize,
}

impl Colouring {
    fn runs(&mut self, language: &str, text: &str) -> Option<Vec<Run>> {
        self.fences += 1;
        self.bytes += text.len();
        let within = self.fences <= FENCES_COLOURED && self.bytes <= PREVIEW_TEXT_BYTES;
        within.then(|| code::fence_runs(language, text)).flatten()
    }
}

impl Page {
    fn take(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text),
            Event::Html(text) => self.html(&text),
            Event::InlineHtml(_) => {}
            Event::Code(text) => self.write(&text, Look::CODE),
            Event::InlineMath(text) => self.write(&text, Look::FORMULA),
            Event::DisplayMath(text) => self.display_formula(&text),
            Event::FootnoteReference(label) => self.write(&label, Look::NOTE),
            Event::SoftBreak => self.text(SOFT_BREAK),
            Event::HardBreak => self.text(HARD_BREAK),
            Event::Rule => {
                self.close_words();
                self.add(Block::Rule);
            }
            Event::TaskListMarker(checked) => self.check(checked),
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.open_words(Words::Paragraph),
            Tag::Heading { level, .. } => self.open_words(Words::Heading(heading_level(level))),
            Tag::TableCell => self.open_words(Words::Cell),
            Tag::BlockQuote(_) => self.hold(Holder::Quote(Vec::new())),
            Tag::List(first) => self.hold(Holder::List {
                first,
                items: Vec::new(),
            }),
            Tag::Item => self.hold(Holder::Item {
                checked: None,
                blocks: Vec::new(),
            }),
            Tag::FootnoteDefinition(label) => self.hold(Holder::Note {
                label: label.as_ref().into(),
                blocks: Vec::new(),
            }),
            Tag::CodeBlock(kind) => self.open_code(language(&kind)),
            Tag::HtmlBlock => self.open_html(),
            Tag::Table(aligns) => self.open_table(&aligns),
            Tag::TableHead => self.with_table(|table| table.in_head = true),
            Tag::TableRow => self.with_table(|table| table.row.clear()),
            Tag::Emphasis => self.looks.push(Look::EMPHASIS),
            Tag::Strong => self.looks.push(Look::STRONG),
            Tag::Strikethrough => self.looks.push(Look::STRUCK),
            Tag::Link { dest_url, .. } => self.links.push(dest_url.as_ref().into()),
            Tag::Image { dest_url, .. } => {
                self.picture = Some((dest_url.as_ref().into(), String::new()));
            }
            Tag::MetadataBlock(_) => self.metadata = true,
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::TableCell => self.close_words(),
            TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item | TagEnd::FootnoteDefinition => {
                self.close_words();
                self.let_go();
            }
            TagEnd::CodeBlock => self.close_code(),
            TagEnd::HtmlBlock => self.close_html(),
            TagEnd::Table => self.close_table(),
            TagEnd::TableHead => self.with_table(|table| table.in_head = false),
            TagEnd::TableRow => self.with_table(|table| {
                let row = mem::take(&mut table.row);
                table.rows.push(row.into());
            }),
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                self.looks.pop();
            }
            TagEnd::Link => {
                self.links.pop();
            }
            TagEnd::Image => self.close_picture(),
            TagEnd::MetadataBlock(_) => self.metadata = false,
            _ => {}
        }
    }

    fn text(&mut self, text: &str) {
        if self.metadata {
            return;
        }
        if let Some((_, code)) = self.code.as_mut() {
            code.push_str(text);
        } else {
            self.write(text, Look::PLAIN);
        }
    }

    fn html(&mut self, text: &str) {
        if let Some(html) = self.html.as_mut() {
            html.push_str(text);
        }
    }

    fn open_html(&mut self) {
        self.close_words();
        self.html = Some(String::new());
    }

    fn close_html(&mut self) {
        let Some(html) = self.html.take() else {
            return;
        };
        let words = html::words(&html);
        if !words.is_empty() {
            self.write(&words, Look::PLAIN);
            self.close_words();
        }
    }

    fn write(&mut self, text: &str, look: Look) {
        if let Some((_, alt)) = self.picture.as_mut() {
            alt.push_str(text);
            return;
        }
        if self.words.is_none() {
            self.words = Some(Words::Paragraph);
        }
        let look = self.looks.iter().fold(look, |all, look| all.with(*look));
        self.writing.push(text, look, self.links.last());
    }

    fn open_words(&mut self, words: Words) {
        self.close_words();
        self.words = Some(words);
    }

    fn close_words(&mut self) {
        let Some(words) = self.words.take() else {
            return;
        };
        let text = self.writing.take();
        match words {
            Words::Paragraph if text.text.trim().is_empty() => {}
            Words::Paragraph => self.add(Block::Paragraph(text)),
            Words::Heading(level) => self.add(Block::Heading { level, text }),
            Words::Cell => self.with_table(|table| {
                if table.in_head {
                    table.head.push(text);
                } else {
                    table.row.push(text);
                }
            }),
        }
    }

    fn display_formula(&mut self, source: &str) {
        if matches!(self.words, Some(Words::Paragraph)) {
            self.close_words();
            self.add(Block::Figure {
                kind: FigureKind::Formula,
                source: source.into(),
            });
            self.words = Some(Words::Paragraph);
        } else {
            self.write(source, Look::FORMULA);
        }
    }

    fn close_picture(&mut self) {
        let Some((target, alt)) = self.picture.take() else {
            return;
        };
        if matches!(self.words, Some(Words::Paragraph) | None) {
            self.close_words();
            let link = self.links.last().cloned();
            self.add(Block::Picture {
                target,
                alt: alt.into(),
                link,
            });
            self.words = Some(Words::Paragraph);
        } else {
            self.write(&alt, Look::PLAIN);
        }
    }

    fn open_code(&mut self, language: Option<String>) {
        self.close_words();
        self.code = Some((language, String::new()));
    }

    fn close_code(&mut self) {
        let Some((language, text)) = self.code.take() else {
            return;
        };
        let figure = |kind| Block::Figure {
            kind,
            source: text.as_str().into(),
        };
        let block = match language.as_deref() {
            Some(DIAGRAM_LANGUAGE) => figure(FigureKind::Diagram),
            Some(FORMULA_LANGUAGE) => figure(FigureKind::Formula),
            Some(language) => code_block(&text, self.colouring.runs(language, &text)),
            None => code_block(&text, None),
        };
        self.add(block);
    }

    fn open_table(&mut self, aligns: &[Alignment]) {
        self.close_words();
        self.tables.push(Table {
            aligns: aligns.iter().map(|align| column_align(*align)).collect(),
            ..Table::default()
        });
    }

    fn with_table(&mut self, change: impl FnOnce(&mut Table)) {
        if let Some(table) = self.tables.last_mut() {
            change(table);
        }
    }

    fn close_table(&mut self) {
        self.close_words();
        let Some(table) = self.tables.pop() else {
            return;
        };
        self.add(Block::Table {
            aligns: table.aligns.into(),
            head: table.head.into(),
            rows: table.rows.into(),
        });
    }

    fn check(&mut self, checked: bool) {
        if let Some(Holder::Item { checked: item, .. }) = self.holders.last_mut() {
            *item = Some(checked);
        }
    }

    fn hold(&mut self, holder: Holder) {
        self.close_words();
        let depth = self
            .holders
            .iter()
            .filter(|holder| !matches!(holder, Holder::Item { .. }))
            .count();
        let too_deep = depth >= DEEPEST;
        let in_flat = matches!(self.holders.last(), Some(Holder::Flat));
        self.holders.push(if too_deep || in_flat {
            Holder::Flat
        } else {
            holder
        });
    }

    fn let_go(&mut self) {
        let Some(holder) = self.holders.pop() else {
            return;
        };
        match holder {
            Holder::Quote(blocks) => self.add(Block::Quote(blocks.into())),
            Holder::List { first, items } => self.add(Block::List {
                first,
                items: items.into(),
            }),
            Holder::Item { checked, blocks } => self.add_item(Item {
                checked,
                blocks: blocks.into(),
            }),
            Holder::Note { label, blocks } => self.add(Block::Note {
                label,
                blocks: blocks.into(),
            }),
            Holder::Flat => {}
        }
    }

    fn add_item(&mut self, item: Item) {
        if let Some(Holder::List { items, .. }) = self.holders.last_mut() {
            items.push(item);
        } else {
            for block in item.blocks.iter() {
                self.add(block.clone());
            }
        }
    }

    fn add(&mut self, block: Block) {
        let held = self
            .holders
            .iter_mut()
            .rev()
            .find_map(|holder| match holder {
                Holder::Quote(blocks)
                | Holder::Item { blocks, .. }
                | Holder::Note { blocks, .. } => Some(blocks),
                Holder::List { .. } | Holder::Flat => None,
            });
        held.unwrap_or(&mut self.root).push(block);
    }

    fn finish(mut self) -> Vec<Block> {
        self.close_code();
        self.close_html();
        self.close_words();
        while !self.tables.is_empty() {
            self.close_table();
        }
        while !self.holders.is_empty() {
            self.close_words();
            self.let_go();
        }
        self.root
    }
}

fn code_block(text: &str, runs: Option<Vec<Run>>) -> Block {
    Block::Code {
        text: text.into(),
        runs: runs.unwrap_or_default().into(),
    }
}

fn language(kind: &CodeBlockKind<'_>) -> Option<String> {
    match kind {
        CodeBlockKind::Fenced(info) => info
            .split_whitespace()
            .next()
            .map(|word| word.trim_matches(['{', '}', '.']).to_ascii_lowercase())
            .filter(|word| !word.is_empty()),
        CodeBlockKind::Indented => None,
    }
}

const fn heading_level(level: HeadingLevel) -> u8 {
    level as u8
}

const fn column_align(align: Alignment) -> Align {
    match align {
        Alignment::None | Alignment::Left => Align::Start,
        Alignment::Center => Align::Center,
        Alignment::Right => Align::End,
    }
}

#[cfg(test)]
mod tests;
