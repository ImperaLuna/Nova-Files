mod blocks;
mod inline;

use std::ops::Range;
use std::sync::Arc;

use gpui::{
    AnyElement, InteractiveElement as _, IntoElement as _, ListAlignment, ListOffset, ListState,
    ParentElement as _, Pixels, SharedString, Size, Styled as _, list, px,
};
use nova_files_preview_api::markdown::Block;
use nova_files_shell::theme::{Colors, radius, size};

use crate::code::Coded;
use crate::pages::Scroll;

pub use blocks::Paint;

const PAGE_WIDTH: Pixels = px(760.0);
const PAGE_PAD_X: Pixels = px(32.0);
const OVERDRAW: Pixels = px(600.0);
const SCROLL_STEP_SHARE: f32 = 0.9;
const CODE_ROW_LINES: usize = 32;
const LINE_END: char = '\n';

pub struct Page {
    pub blocks: Arc<[Block]>,
    pub rows: Arc<[Row]>,
    pub list: ListState,
}

pub enum Row {
    Block(usize),
    Code {
        coded: Arc<Coded>,
        lines: Range<usize>,
    },
}

impl Page {
    pub fn new(blocks: Arc<[Block]>, mono: &SharedString) -> Self {
        let rows: Arc<[Row]> = rows(&blocks, mono).into();
        let list = ListState::new(rows.len(), ListAlignment::Top, OVERDRAW);
        Self { blocks, rows, list }
    }

    pub fn carrying(self, before: Option<Self>) -> Self {
        if let Some(before) = before {
            self.list.scroll_to(before.list.logical_scroll_top());
        }
        self
    }

    pub fn scroll(&self, to: Scroll, room: Size<Pixels>) {
        let step = room.height * SCROLL_STEP_SHARE;
        match to {
            Scroll::Up => self.list.scroll_by(-step),
            Scroll::Down => self.list.scroll_by(step),
            Scroll::First => self.list.scroll_to(ListOffset::default()),
            Scroll::Last => self.list.scroll_to_end(),
        }
    }
}

pub fn page_art(
    page: &Page,
    mono: SharedString,
    room: Size<Pixels>,
    colors: &Colors,
) -> AnyElement {
    let (blocks, rows) = (Arc::clone(&page.blocks), Arc::clone(&page.rows));
    let paint = Paint {
        colors: *colors,
        mono,
    };
    gpui::div()
        .debug_selector(|| "preview-markdown".to_owned())
        .flex()
        .flex_col()
        .w(PAGE_WIDTH.min(room.width))
        .h(room.height)
        .overflow_hidden()
        .rounded(radius::CARD)
        .border(size::LINE)
        .border_color(colors.line)
        .bg(colors.window)
        .px(PAGE_PAD_X)
        .child(
            list(page.list.clone(), move |index, _, _| {
                let place = blocks::Place {
                    first: index == 0,
                    last: index + 1 == rows.len(),
                };
                match rows.get(index) {
                    Some(Row::Block(block)) => blocks.get(*block).map_or_else(
                        || gpui::div().into_any_element(),
                        |block| blocks::top_block(block, place, &paint),
                    ),
                    Some(Row::Code { coded, lines }) => {
                        blocks::code_rows(coded, lines.clone(), place, &paint)
                    }
                    None => gpui::div().into_any_element(),
                }
            })
            .w_full()
            .flex_1(),
        )
        .into_any_element()
}

fn rows(blocks: &[Block], mono: &SharedString) -> Vec<Row> {
    let mut rows = Vec::with_capacity(blocks.len());
    for (index, block) in blocks.iter().enumerate() {
        let code = match block {
            Block::Code { text, runs } => Some((text, &**runs)),
            Block::Figure { source, .. } => Some((source, &[][..])),
            _ => None,
        };
        let Some((text, runs)) = code else {
            rows.push(Row::Block(index));
            continue;
        };
        let coded = Arc::new(Coded::new(
            text.trim_end_matches(LINE_END),
            runs,
            mono.clone(),
        ));
        let count = coded.lines.len();
        rows.extend((0..count).step_by(CODE_ROW_LINES).map(|start| Row::Code {
            coded: Arc::clone(&coded),
            lines: start..(start + CODE_ROW_LINES).min(count),
        }));
    }
    rows
}
