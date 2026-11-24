use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::sync::Arc;

use gpui::{Pixels, RenderImage, ScrollHandle, Size, point, px};
use nova_files_preview_api::{PageSize, PreviewId};

const PAGE_WIDTH: Pixels = px(860.0);
const PAGE_GAP: Pixels = px(12.0);
const HALF: f32 = 0.5;
const KEPT_SCREENS: f32 = 1.0;

pub struct Pages {
    pub id: PreviewId,
    pub sizes: Arc<[PageSize]>,
    pub scroll: ScrollHandle,
    pub drawn: BTreeMap<usize, Arc<RenderImage>>,
    pub asked: BTreeSet<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scroll {
    Up,
    Down,
    First,
    Last,
}

pub struct Layout {
    pub width: Pixels,
    pub tops: Vec<Pixels>,
    pub heights: Vec<Pixels>,
    pub total: Pixels,
}

impl Pages {
    pub fn new(id: PreviewId, sizes: Arc<[PageSize]>) -> Self {
        Self {
            id,
            sizes,
            scroll: ScrollHandle::new(),
            drawn: BTreeMap::new(),
            asked: BTreeSet::new(),
        }
    }

    pub fn scrolled(&self) -> Pixels {
        -self.scroll.offset().y
    }

    pub fn images(self) -> impl Iterator<Item = Arc<RenderImage>> {
        self.drawn.into_values()
    }

    pub fn scroll(&self, to: Scroll, room: Size<Pixels>) {
        let layout = Layout::of(&self.sizes, room);
        let view = room.height.min(layout.total);
        let last = (layout.total - view).max(px(0.0));
        let step = (view - PAGE_GAP).max(PAGE_GAP);
        let scrolled = match to {
            Scroll::Up => self.scrolled() - step,
            Scroll::Down => self.scrolled() + step,
            Scroll::First => px(0.0),
            Scroll::Last => last,
        };
        self.scroll
            .set_offset(point(px(0.0), -scrolled.clamp(px(0.0), last)));
    }

    pub fn reading(&self, room: Size<Pixels>) -> usize {
        Layout::of(&self.sizes, room).at(self.scrolled(), room.height)
    }
}

impl Layout {
    pub fn of(sizes: &[PageSize], room: Size<Pixels>) -> Self {
        let width = room.width.min(PAGE_WIDTH);
        let mut tops = Vec::with_capacity(sizes.len());
        let mut heights = Vec::with_capacity(sizes.len());
        let mut top = px(0.0);
        for size in sizes {
            #[expect(
                clippy::cast_precision_loss,
                reason = "a page side in points, a few thousand at most"
            )]
            let ratio = size.height as f32 / size.width.max(1) as f32;
            let height = width * ratio;
            tops.push(top);
            heights.push(height);
            top += height + PAGE_GAP;
        }
        Self {
            width,
            tops,
            heights,
            total: (top - PAGE_GAP).max(px(0.0)),
        }
    }

    pub fn near(&self, scrolled: Pixels, view: Pixels) -> Range<usize> {
        let kept = view * KEPT_SCREENS;
        self.between(scrolled - kept, scrolled + view + kept)
    }

    pub fn at(&self, scrolled: Pixels, view: Pixels) -> usize {
        let line = scrolled + view * HALF;
        let at_end = scrolled + view >= self.total;
        let last = self.tops.len().saturating_sub(1);
        if at_end {
            return last;
        }
        self.tops
            .partition_point(|top| *top <= line)
            .saturating_sub(1)
            .min(last)
    }

    fn between(&self, from: Pixels, to: Pixels) -> Range<usize> {
        let start = self
            .tops
            .iter()
            .zip(&self.heights)
            .position(|(top, height)| *top + *height >= from)
            .unwrap_or(self.tops.len());
        let end = self.tops.partition_point(|top| *top < to);
        start..end.max(start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LETTER: PageSize = PageSize {
        width: 600,
        height: 800,
    };

    fn room() -> Size<Pixels> {
        gpui::size(px(300.0), px(500.0))
    }

    #[test]
    fn pages_stack_at_the_room_width_with_a_gap() {
        let layout = Layout::of(&[LETTER, LETTER, LETTER], room());
        assert_eq!(layout.width, px(300.0));
        assert_eq!(layout.heights, vec![px(400.0); 3]);
        assert_eq!(layout.tops, vec![px(0.0), px(412.0), px(824.0)]);
        assert_eq!(layout.total, px(1224.0));
        let wide = Layout::of(&[LETTER], gpui::size(px(4000.0), px(500.0)));
        assert_eq!(wide.width, PAGE_WIDTH);
    }

    #[test]
    fn pages_on_screen_and_a_screen_either_side_are_near() {
        let layout = Layout::of(&[LETTER; 10], room());
        assert_eq!(layout.near(px(0.0), px(500.0)), 0..3);
        assert_eq!(layout.near(px(2060.0), px(500.0)), 3..8);
        assert_eq!(layout.near(px(4000.0), px(500.0)), 8..10);
    }

    #[test]
    fn the_page_read_is_the_one_across_the_middle_or_the_last_at_the_end() {
        let layout = Layout::of(&[LETTER; 3], room());
        assert_eq!(layout.at(px(0.0), px(500.0)), 0);
        assert_eq!(layout.at(px(200.0), px(500.0)), 1);
        assert_eq!(layout.at(px(724.0), px(500.0)), 2);
        assert_eq!(Layout::of(&[], room()).at(px(0.0), px(500.0)), 0);
    }

    #[test]
    fn page_keys_scroll_a_screen_and_stop_at_either_end() {
        let pages = Pages::new(PreviewId(1), Arc::from([LETTER; 3]));
        pages.scroll(Scroll::Down, room());
        assert_eq!(pages.scrolled(), px(488.0));
        pages.scroll(Scroll::Down, room());
        assert_eq!(pages.scrolled(), px(724.0));
        pages.scroll(Scroll::Up, room());
        assert_eq!(pages.scrolled(), px(236.0));
        pages.scroll(Scroll::First, room());
        assert_eq!(pages.scrolled(), px(0.0));
        pages.scroll(Scroll::Last, room());
        assert_eq!(pages.reading(room()), 2);
    }
}
