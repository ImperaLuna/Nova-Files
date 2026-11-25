mod blocks;
mod inline;
mod pictures;

use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    AnyElement, App, InteractiveElement as _, IntoElement as _, ListAlignment, ListOffset,
    ListState, ParentElement as _, Pixels, RenderImage, SharedString, Size, Styled as _, Window,
    list, px,
};
use nova_files_preview_api::markdown::Block;
use nova_files_shell::theme::{Colors, radius, size};

use crate::code::Coded;
use crate::links::slug;
use crate::pages::Scroll;

pub use blocks::Paint;
pub use pictures::Pictures;

const PAGE_WIDTH: Pixels = px(760.0);
const PAGE_PAD_X: Pixels = px(32.0);
pub const PICTURE_ROOM: Pixels = px(696.0);
pub const PICTURE_TALL: u32 = 2;
const BOTH_SIDES: f32 = 2.0;
const OVERDRAW: Pixels = px(600.0);
const SCROLL_STEP_SHARE: f32 = 0.9;
const CODE_ROW_LINES: usize = 32;
const LINE_END: char = '\n';

pub type Follow = Rc<dyn Fn(&Arc<str>, &mut Window, &mut App)>;

pub struct Page {
    pub blocks: Arc<[Block]>,
    pub rows: Arc<[Row]>,
    pub list: ListState,
    pub pictures: Pictures,
    anchors: HashMap<String, usize>,
}

pub enum Row {
    Block(usize),
    Code {
        coded: Arc<Coded>,
        lines: Range<usize>,
    },
}

impl Page {
    pub fn new(blocks: Arc<[Block]>, mono: &SharedString, folder: Option<&Path>) -> Self {
        let rows: Arc<[Row]> = rows(&blocks, mono).into();
        let list = ListState::new(rows.len(), ListAlignment::Top, OVERDRAW);
        let anchors = anchors(&blocks, &rows);
        let pictures = Pictures::new(&blocks, folder);
        Self {
            blocks,
            rows,
            list,
            pictures,
            anchors,
        }
    }

    pub fn go_to(&self, anchor: &str) -> bool {
        let row = self
            .anchors
            .get(anchor)
            .or_else(|| self.anchors.get(&slug(anchor)));
        if let Some(&item_ix) = row {
            self.list.scroll_to(ListOffset {
                item_ix,
                offset_in_item: Pixels::ZERO,
            });
        }
        row.is_some()
    }

    pub fn carrying(self, before: Option<Self>) -> (Self, Vec<Arc<RenderImage>>) {
        let Some(before) = before else {
            return (self, Vec::new());
        };
        self.list.scroll_to(before.list.logical_scroll_top());
        (self, before.images().collect())
    }

    pub fn images(self) -> impl Iterator<Item = Arc<RenderImage>> {
        self.pictures.images()
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
    (mono, follow): (SharedString, Follow),
    (room, scale_factor): (Size<Pixels>, f32),
    colors: &Colors,
) -> AnyElement {
    let (blocks, rows) = (Arc::clone(&page.blocks), Arc::clone(&page.rows));
    let width = PAGE_WIDTH.min(room.width);
    let paint = Paint {
        colors: *colors,
        mono,
        follow,
        pictures: Rc::clone(&page.pictures.drawn),
        room: (
            (width - PAGE_PAD_X * BOTH_SIDES).max(Pixels::ZERO),
            scale_factor,
        ),
    };
    gpui::div()
        .debug_selector(|| "preview-markdown".to_owned())
        .flex()
        .flex_col()
        .w(width)
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

fn anchors(blocks: &[Block], rows: &[Row]) -> HashMap<String, usize> {
    let mut anchors = HashMap::new();
    for (row, block) in rows
        .iter()
        .enumerate()
        .filter_map(|(row, shown)| match shown {
            Row::Block(block) => Some((row, blocks.get(*block)?)),
            Row::Code { .. } => None,
        })
    {
        let Block::Heading { text, .. } = block else {
            continue;
        };
        let base = slug(&text.text);
        let mut anchor = base.clone();
        let mut again = 0_usize;
        while anchors.contains_key(&anchor) {
            again += 1;
            anchor = format!("{base}-{again}");
        }
        anchors.insert(anchor, row);
    }
    anchors
}

#[cfg(test)]
mod tests {
    use nova_files_preview_api::markdown::Inline;

    use super::*;

    fn heading(words: &str) -> Block {
        Block::Heading {
            level: 2,
            text: Inline {
                text: words.into(),
                spans: Arc::new([]),
            },
        }
    }

    #[test]
    fn headings_are_found_by_their_slug_and_repeats_are_counted() {
        let blocks = [
            heading("Install"),
            Block::Rule,
            heading("Install"),
            heading("Part Two"),
        ];
        let rows: Vec<Row> = (0..blocks.len()).map(Row::Block).collect();
        let anchors = anchors(&blocks, &rows);
        assert_eq!(anchors.get("install"), Some(&0));
        assert_eq!(anchors.get("install-1"), Some(&2));
        assert_eq!(anchors.get("part-two"), Some(&3));
        assert_eq!(anchors.len(), 3);
    }
}
