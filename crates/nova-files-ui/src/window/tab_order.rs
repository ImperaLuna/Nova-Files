//! Reordering tabs: a tab's row in the Open list dragged up or down (across,
//! in a narrow window) lands between two rows, where a line shows. The
//! pointer over a row's first half puts the tab before it, over its second
//! half after it. Where the tab would stay put, no line shows and a drop
//! does nothing. The tab on screen stays on screen wherever it goes.
//!
//! Dragging a tab carries which tab it is, never files: nothing that takes
//! files takes it, and it opens nothing it is held over (see
//! `hover_open`), nor any other window's Open list. Let go away from its
//! own list, it stays where it was.

use std::mem;

use gpui::{App, Context, EntityId, IntoElement, Render, SharedString, Window};

use super::MainWindow;
use super::dragging::chip;

/// What a drag of a tab's row carries.
#[derive(Debug, Clone)]
pub struct DraggedTab {
    /// The window whose tab it is: no other window's list takes it.
    window: EntityId,
    index: usize,
    name: SharedString,
}

impl DraggedTab {
    /// Tab `index` of `window`, named `name`.
    pub const fn new(window: EntityId, index: usize, name: SharedString) -> Self {
        Self {
            window,
            index,
            name,
        }
    }

    /// Which tab it is.
    pub const fn index(&self) -> usize {
        self.index
    }
}

/// The chip under the pointer while a tab is dragged: its name.
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

/// Which side of a row a dragged tab would land on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Before,
    After,
}

/// The tab dragged, and the row it is over with the side it would land
/// on.
#[derive(Debug, Default)]
pub struct TabOrder {
    dragged: usize,
    over: Option<(usize, Side)>,
    /// Whether the move under way is a tab's (see
    /// [`MainWindow::tab_drag_heard`]).
    heard: bool,
}

impl MainWindow {
    /// Tab `dragged` moved: inside tab `row`'s row, on `side` of it, or
    /// not. A tab of another window is none of this list's business.
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

    /// The side of tab `row`'s row where the tab dragged would land; none
    /// when no tab is dragged over that row, or it would stay put.
    pub(crate) fn tab_landing_line(&self, row: usize, cx: &App) -> Option<Side> {
        let (at, side) = self.tab_order.over.filter(|(at, _)| *at == row)?;
        if !cx.has_active_drag() {
            return None;
        }
        landing(self.tab_order.dragged, slot(at, side)).map(|_| side)
    }

    /// Whether the pointer's move, which the window's root hears last,
    /// was a tab's. Any other move lets go of where a tab would land, so
    /// no line stays from a tab let go elsewhere.
    pub(crate) fn tab_drag_heard(&mut self, cx: &mut Context<Self>) -> bool {
        let heard = mem::take(&mut self.tab_order.heard);
        if !heard && self.tab_order.over.take().is_some() {
            cx.notify();
        }
        heard
    }

    /// Tab `dragged` was let go over the Open list: it moves where the
    /// line showed.
    pub(crate) fn tab_dropped(&mut self, dragged: &DraggedTab, cx: &mut Context<Self>) {
        let Some((at, side)) = self.tab_order.over.take() else {
            return;
        };
        if dragged.window == cx.entity_id() {
            self.move_tab(dragged.index, slot(at, side), cx);
        }
    }

    /// Moves tab `from` to `slot`, between the tabs `slot - 1` and `slot`
    /// as they are now.
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

/// The slot on `side` of row `at`.
const fn slot(at: usize, side: Side) -> usize {
    match side {
        Side::Before => at,
        Side::After => at + 1,
    }
}

/// Where tab `from` ends up dropped at `slot`; none when that is where it
/// is.
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
        // Tabs 0 1 2 3; tab 1 dragged.
        assert_eq!(landing(1, 0), Some(0));
        assert_eq!(landing(1, 3), Some(2));
        assert_eq!(landing(1, 4), Some(3));
        // On either side of itself it stays.
        assert_eq!(landing(1, 1), None);
        assert_eq!(landing(1, 2), None);
        assert_eq!(slot(2, Side::Before), 2);
        assert_eq!(slot(2, Side::After), 3);
    }
}
