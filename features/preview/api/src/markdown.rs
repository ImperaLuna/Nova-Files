//! A Markdown file read into blocks, for a quick look to draw as a page.

use std::sync::Arc;

use crate::Run;

/// The most bytes of a Markdown file a preview reads: more than most
/// documents hold, few enough to read and lay out at once.
pub const PREVIEW_MARKDOWN_BYTES: usize = 256 * 1024;

/// One block of a Markdown page, in reading order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// A heading, `level` 1 (largest) to 6.
    Heading {
        level: u8,
        text: Inline,
    },
    Paragraph(Inline),
    /// A quote, holding its own blocks.
    Quote(Arc<[Self]>),
    /// A list: numbered from `first` when it has one, bulleted otherwise.
    List {
        first: Option<u64>,
        items: Arc<[Item]>,
    },
    /// A fenced or indented code block; `runs` colour it when its language
    /// is known and are empty otherwise.
    Code {
        text: Arc<str>,
        runs: Arc<[Run]>,
    },
    /// A table: one alignment and one header cell per column, then its
    /// rows, each with at most that many cells.
    Table {
        aligns: Arc<[Align]>,
        head: Arc<[Inline]>,
        rows: Arc<[Arc<[Inline]>]>,
    },
    /// A horizontal rule.
    Rule,
    /// A picture on its own line: `target` as the file wrote it, the words
    /// that stand for it, and where it links to when it is a link.
    Picture {
        target: Arc<str>,
        alt: Arc<str>,
        link: Option<Arc<str>>,
    },
    /// A diagram or formula the page draws from its source.
    Figure {
        kind: FigureKind,
        source: Arc<str>,
    },
    /// A footnote's text, named by the `label` its references use.
    Note {
        label: Arc<str>,
        blocks: Arc<[Self]>,
    },
}

/// One entry of a [`Block::List`]; `checked` is set for a task list's
/// entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub checked: Option<bool>,
    pub blocks: Arc<[Block]>,
}

/// How a table column lines up its cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
}

/// What a [`Block::Figure`] holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FigureKind {
    /// A ```` ```mermaid ```` fence.
    Diagram,
    /// A `$$` formula on its own line.
    Formula,
}

/// Text inside a block: its words, and `spans` saying how each piece of
/// them looks, in order. The spans' lengths add up to `text`'s.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Inline {
    pub text: Arc<str>,
    pub spans: Arc<[Span]>,
}

/// One piece of an [`Inline`] text: `bytes` long, with one look, and the
/// target it links to, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub bytes: u32,
    pub look: Look,
    pub link: Option<Arc<str>>,
}

/// How a piece of text looks: any mix of [`Look::STRONG`],
/// [`Look::EMPHASIS`], [`Look::STRUCK`], [`Look::CODE`], [`Look::FORMULA`]
/// and [`Look::NOTE`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Look(u8);

impl Look {
    pub const PLAIN: Self = Self(0);
    pub const STRONG: Self = Self(1);
    pub const EMPHASIS: Self = Self(1 << 1);
    pub const STRUCK: Self = Self(1 << 2);
    pub const CODE: Self = Self(1 << 3);
    /// A `$…$` formula inside a line, holding its source.
    pub const FORMULA: Self = Self(1 << 4);
    /// A reference to a [`Block::Note`], holding its label.
    pub const NOTE: Self = Self(1 << 5);

    /// This look with `other`'s added.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether this look holds all of `other`.
    pub const fn has(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}
