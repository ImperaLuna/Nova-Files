use std::time::Duration;

use gpui::{Context, Pixels, Point, Task, point, px};

use crate::theme::size;
use crate::window::MainWindow;

const EDGE: Pixels = px(32.0);
const STEP: Pixels = px(6.0);
const FASTEST: f32 = 6.0;
const TICK: Duration = Duration::from_millis(16);

#[derive(Default)]
pub struct EdgeScroll {
    pointer: Option<(Point<Pixels>, bool)>,
    ticking: Option<Task<()>>,
}

impl MainWindow {
    pub(crate) fn edge_pointer(
        &mut self,
        pointer: Option<Point<Pixels>>,
        anywhere_across: bool,
        cx: &Context<Self>,
    ) {
        self.edge.pointer = pointer.map(|at| (at, anywhere_across));
        if self.edge.pointer.is_none() {
            self.edge.ticking = None;
            return;
        }
        if self.edge.ticking.is_some() {
            return;
        }
        self.edge.ticking = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                let going = this.update(cx, Self::edge_tick).unwrap_or(false);
                if !going {
                    break;
                }
            }
        }));
    }

    pub(crate) fn pointer_carried(&mut self, pointer: Option<Point<Pixels>>, cx: &Context<Self>) {
        if !self.band_dragged() {
            self.edge_pointer(pointer, false, cx);
        }
    }

    fn edge_tick(&mut self, cx: &mut Context<Self>) -> bool {
        let Some((at, anywhere_across)) = self.edge.pointer else {
            return false;
        };
        let list = self.list_bounds();
        let across = at.x >= list.left() && at.x <= list.right();
        let by = edge_step(at.y, (list.top(), list.bottom() - size::ISLAND_CLEARANCE));
        if by == px(0.0) || !(anywhere_across || across) {
            return true;
        }
        let handle = self.scroll.0.borrow().base_handle.clone();
        let (offset, most) = (handle.offset(), handle.max_offset());
        let y = (offset.y - by).clamp(-most.y, px(0.0));
        if y != offset.y {
            handle.set_offset(point(offset.x, y));
            self.band_scrolled(cx);
            cx.notify();
        }
        true
    }
}

fn edge_step(y: Pixels, (top, bottom): (Pixels, Pixels)) -> Pixels {
    let depth = if y < top + EDGE {
        -(top + EDGE - y)
    } else if y > bottom - EDGE {
        y - (bottom - EDGE)
    } else {
        return px(0.0);
    };
    let speed = (depth / EDGE).clamp(-FASTEST, FASTEST);
    STEP * speed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_step_grows_with_the_distance_into_an_edge() {
        let list = (px(100.0), px(500.0));
        assert_eq!(edge_step(px(300.0), list), px(0.0));
        assert_eq!(edge_step(px(500.0), list), STEP);
        assert_eq!(edge_step(px(100.0), list), -STEP);
        assert_eq!(edge_step(px(532.0), list), STEP * 2.0);
        assert_eq!(edge_step(px(5000.0), list), STEP * FASTEST);
        assert_eq!(edge_step(px(-5000.0), list), -STEP * FASTEST);
    }
}
