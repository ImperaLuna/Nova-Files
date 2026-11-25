mod blocks;
mod inline;

use std::sync::Arc;

use gpui::{
    AnyElement, InteractiveElement as _, IntoElement as _, ListAlignment, ListOffset,
    ListSizingBehavior, ListState, ParentElement as _, Pixels, SharedString, Size, Styled as _,
    list, px,
};
use nova_files_preview_api::markdown::Block;
use nova_files_shell::theme::{Colors, radius, size};

use crate::pages::Scroll;

pub use blocks::Paint;

const PAGE_WIDTH: Pixels = px(760.0);
const PAGE_PAD_X: Pixels = px(32.0);
const OVERDRAW: Pixels = px(600.0);
const SCROLL_STEP_SHARE: f32 = 0.9;

pub struct Page {
    pub blocks: Arc<[Block]>,
    pub list: ListState,
}

impl Page {
    pub fn new(blocks: Arc<[Block]>) -> Self {
        let list = ListState::new(blocks.len(), ListAlignment::Top, OVERDRAW);
        Self { blocks, list }
    }

    pub fn carrying(self, before: Option<Self>) -> Self {
        if let Some(before) = before.filter(|before| before.blocks.len() == self.blocks.len()) {
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
    let blocks = Arc::clone(&page.blocks);
    let paint = Paint {
        colors: *colors,
        mono,
    };
    gpui::div()
        .debug_selector(|| "preview-markdown".to_owned())
        .flex()
        .flex_col()
        .w(PAGE_WIDTH.min(room.width))
        .max_h(room.height)
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
                    last: index + 1 == blocks.len(),
                };
                blocks.get(index).map_or_else(
                    || gpui::div().into_any_element(),
                    |block| blocks::top_block(block, place, &paint),
                )
            })
            .with_sizing_behavior(ListSizingBehavior::Infer)
            .w_full()
            .max_h(room.height),
        )
        .into_any_element()
}
