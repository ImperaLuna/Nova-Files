use std::mem;

use gpui::{App, Context, EntityId, IntoElement, Render, SharedString, Window};

use super::MainWindow;
use super::dragging::chip;

#[derive(Debug, Clone)]
pub struct DraggedTab {
    window: EntityId,
    index: usize,
    name: SharedString,
}

impl DraggedTab {
    pub const fn new(window: EntityId, index: usize, name: SharedString) -> Self {
        Self {
            window,
            index,
            name,
        }
    }

    pub const fn index(&self) -> usize {
        self.index
    }
}

pub struct TabChip {
    name: SharedString,
}

impl TabChip {
    pub fn new(tab: &DraggedTab) -> Self {
        Self {
            name: tab.name.clone(),
        }
    }
}

impl Render for TabChip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        chip(self.name.clone(), cx)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Before,
    After,
}

#[derive(Debug, Default)]
pub struct TabOrder {
    dragged: usize,
    over: Option<(usize, Side)>,
    heard: bool,
}

impl MainWindow {
    pub(crate) fn tab_dragged_over(
        &mut self,
        dragged: &DraggedTab,
        (row, inside, side): (usize, bool, Side),
        cx: &mut Context<Self>,
    ) {
        if dragged.window != cx.entity_id() {
            return;
        }
        self.tab_order.heard = true;
        self.tab_order.dragged = dragged.index;
        let over = if inside {
            Some((row, side))
        } else if self.tab_order.over.is_some_and(|(at, _)| at == row) {
            None
        } else {
            return;
        };
        if self.tab_order.over != over {
            self.tab_order.over = over;
            cx.notify();
        }
    }

    pub(crate) fn tab_landing_line(&self, row: usize, cx: &App) -> Option<Side> {
        let (at, side) = self.tab_order.over.filter(|(at, _)| *at == row)?;
        if !cx.has_active_drag() {
            return None;
        }
        landing(self.tab_order.dragged, slot(at, side)).map(|_| side)
    }

    pub(crate) fn tab_drag_heard(&mut self, cx: &mut Context<Self>) -> bool {
        let heard = mem::take(&mut self.tab_order.heard);
        if !heard && self.tab_order.over.take().is_some() {
            cx.notify();
        }
        heard
    }

    pub(crate) fn tab_dropped(&mut self, dragged: &DraggedTab, cx: &mut Context<Self>) {
        let Some((at, side)) = self.tab_order.over.take() else {
            return;
        };
        if dragged.window == cx.entity_id() {
            self.move_tab(dragged.index, slot(at, side), cx);
        }
    }

    pub fn move_tab(&mut self, from: usize, slot: usize, cx: &mut Context<Self>) {
        if from >= self.tab_count() || slot > self.tab_count() {
            return;
        }
        self.tab_order.over = None;
        if let Some(to) = landing(from, slot) {
            self.reorder_tab(from, to);
            cx.notify();
        }
    }
}

const fn slot(at: usize, side: Side) -> usize {
    match side {
        Side::Before => at,
        Side::After => at + 1,
    }
}

const fn landing(from: usize, slot: usize) -> Option<usize> {
    if slot > from + 1 {
        Some(slot - 1)
    } else if slot < from {
        Some(slot)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tab_lands_between_the_rows_it_is_dropped_between() {
        assert_eq!(landing(1, 0), Some(0));
        assert_eq!(landing(1, 3), Some(2));
        assert_eq!(landing(1, 4), Some(3));
        assert_eq!(landing(1, 1), None);
        assert_eq!(landing(1, 2), None);
        assert_eq!(slot(2, Side::Before), 2);
        assert_eq!(slot(2, Side::After), 3);
    }
}
