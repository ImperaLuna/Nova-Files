//! Tabs: several places open in one main window, drawn as the sidebar's
//! Open list (see `view/sidebar/open`). A new tab opens on Start, or on a
//! folder middle-clicked; the last tab can't be closed.
//!
//! The tab on screen keeps its state in the window's own fields, as a
//! window without tabs would, so nothing else needs to know about tabs.
//! Leaving a tab stashes what is its own in a [`Tab`]: its history (and
//! with it where it is), its listing, selection, order, search and scroll
//! position. What only the screen needs is let go: details, thumbnails,
//! the rows the core serves, a quick look, a menu, a rename, a typed path.
//!
//! Coming back puts the stash on screen at once, so the list shows at its
//! scroll position with its selection, then asks the core for it again,
//! since the core serves the rows of one listing per window. That answer
//! arrives in place: the scroll stays and the selection is kept by name
//! (see `events`), so it also brings in what changed meanwhile. A folder
//! still on its way when the tab was left is asked for again; a search is
//! run again.
//!
//! The keys go back to the window on each change of tab, once it draws: a
//! field of the tab left (its search box, a rename) may have held them.
//!
//! A tab in the background holds its listing's names and nothing per row
//! beyond them.

use std::collections::HashMap;
use std::mem;
use std::path::PathBuf;

use gpui::{Context, SharedString, UniformListScrollHandle, Window};
use nova_files_api::{Listing, Order};

use super::rows::release;
use super::search::Search;
use super::{MainWindow, ROOT_FOLDER};
use crate::history::History;
use crate::jump::TypeJump;
use crate::location::Location;
use crate::selection::Selection;

/// How many tabs a window keeps at least: the last can't be closed.
const LAST: usize = 1;

/// What a tab keeps while another is on screen.
pub struct Tab {
    history: History,
    listing: Option<Listing>,
    selection: Option<Selection>,
    order: Order,
    shown_order: Order,
    scroll: UniformListScrollHandle,
    search: Search,
    counted: SharedString,
    /// The folder asked for that hadn't arrived when the tab was left.
    on_its_way: Option<PathBuf>,
}

impl Tab {
    /// Where the tab is going, else where it is.
    fn place(&self) -> Option<Location> {
        self.on_its_way
            .clone()
            .map(Location::Folder)
            .or_else(|| self.history.here().cloned())
    }
}

/// A window's tabs.
pub struct Tabs {
    /// Every tab, in the Open list's order. The one on screen is `None`:
    /// its state is the window's.
    list: Vec<Option<Tab>>,
    current: usize,
    /// The tab on screen changed since the window last drew.
    changed: bool,
}

impl Default for Tabs {
    fn default() -> Self {
        Self {
            list: vec![None],
            current: 0,
            changed: false,
        }
    }
}

impl MainWindow {
    /// How many tabs the window has.
    pub const fn tab_count(&self) -> usize {
        self.tabs.list.len()
    }

    /// The tab on screen, counted from 0.
    pub const fn current_tab(&self) -> usize {
        self.tabs.current
    }

    /// Where each tab is going, else where it is, in order; `None` for
    /// one that has been nowhere yet and isn't going anywhere.
    pub fn tab_places(&self) -> Vec<Option<Location>> {
        self.tabs
            .list
            .iter()
            .map(|tab| {
                tab.as_ref()
                    .map_or_else(|| self.place_on_screen(), Tab::place)
            })
            .collect()
    }

    /// Where the tab on screen is going, else where it is.
    fn place_on_screen(&self) -> Option<Location> {
        self.asked
            .and_then(|_| self.requested.clone())
            .map(Location::Folder)
            .or_else(|| self.location().cloned())
    }

    /// Opens a new tab at `place`, after the others, and shows it.
    pub fn new_tab(&mut self, place: &Location, cx: &mut Context<Self>) {
        if self.pick.is_some() {
            return;
        }
        let left = self.leave_tab(cx);
        self.tabs.list[self.tabs.current] = Some(left);
        self.tabs.list.push(None);
        self.tabs.current = self.tabs.list.len() - 1;
        // A new tab has been nowhere, as a new window: one that can't
        // read its folder falls back to home, then the root.
        self.fallbacks = vec![PathBuf::from(ROOT_FOLDER), self.home.clone()];
        match place {
            Location::Start => self.open_start(cx),
            Location::Folder(folder) => self.open(folder),
        }
        cx.notify();
    }

    /// Opens `folder` in a new tab.
    pub fn open_in_new_tab(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        self.new_tab(&Location::Folder(folder), cx);
    }

    /// Shows tab `index`.
    pub fn switch_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index == self.tabs.current || index >= self.tabs.list.len() {
            return;
        }
        let left = self.leave_tab(cx);
        self.tabs.list[self.tabs.current] = Some(left);
        self.tabs.current = index;
        self.enter_tab(cx);
    }

    /// Shows the tab after the one on screen, the first after the last.
    pub fn next_tab(&mut self, cx: &mut Context<Self>) {
        let count = self.tab_count();
        self.switch_tab((self.tabs.current + 1) % count, cx);
    }

    /// Shows the tab before the one on screen, the last before the first.
    pub fn previous_tab(&mut self, cx: &mut Context<Self>) {
        let count = self.tab_count();
        self.switch_tab((self.tabs.current + count - 1) % count, cx);
    }

    /// Closes tab `index`, unless it is the last one. Closing the tab on
    /// screen shows the one after it, or before it when it was the last.
    pub fn close_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.tab_count() <= LAST || index >= self.tab_count() {
            return;
        }
        if index == self.tabs.current {
            drop(self.leave_tab(cx));
            self.tabs.list.remove(index);
            self.tabs.current = index.min(self.tab_count() - 1);
            self.enter_tab(cx);
        } else {
            self.tabs.list.remove(index);
            if index < self.tabs.current {
                self.tabs.current -= 1;
            }
            cx.notify();
        }
    }

    /// Closes the tab on screen, unless it is the last one.
    pub fn close_current_tab(&mut self, cx: &mut Context<Self>) {
        self.close_tab(self.tabs.current, cx);
    }

    /// Gives the keys back to the window after a change of tab.
    pub(crate) fn settle_tab_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if mem::take(&mut self.tabs.changed) {
            window.focus(&self.keys_home(cx), cx);
        }
    }

    /// Takes the tab on screen's state out of the window, letting go of
    /// what only the screen needs. The window is left as a fresh one.
    fn leave_tab(&mut self, cx: &mut Context<Self>) -> Tab {
        self.forget_rows();
        self.close_preview(None, cx);
        self.close_menu(cx);
        self.close_path_menu();
        self.stop_typing_path(None, cx);
        self.renaming = None;
        self.details = HashMap::new();
        release(self.thumbnails.take_all(), None, cx);
        self.jump = TypeJump::default();
        self.band = None;
        self.pressed_name = false;
        self.island.confirm = None;
        self.falling_back = false;
        self.search.asked = None;
        self.tabs.changed = true;
        let on_its_way = self.asked.take().and_then(|_| self.requested.take());
        Tab {
            history: mem::take(&mut self.history),
            listing: self.listing.take(),
            selection: self.selection.take(),
            order: mem::take(&mut self.order),
            shown_order: mem::take(&mut self.shown_order),
            scroll: mem::replace(&mut self.scroll, UniformListScrollHandle::new()),
            search: mem::replace(&mut self.search, Search::new(cx)),
            counted: mem::take(&mut self.counted),
            on_its_way,
        }
    }

    /// Puts the current tab's stash on screen and asks the core for what
    /// it shows (see the module's comment).
    fn enter_tab(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.list[self.tabs.current].take() else {
            return;
        };
        self.history = tab.history;
        self.listing = tab.listing;
        self.selection = tab.selection;
        self.order = tab.order;
        self.asked_order = tab.order;
        self.shown_order = tab.shown_order;
        self.scroll = tab.scroll;
        self.search = tab.search;
        self.counted = tab.counted;
        // The core serves another listing's rows now.
        self.core_moved = true;
        if let Some(folder) = tab.on_its_way {
            self.request(&folder);
        } else if self.searching() {
            self.find(cx);
        } else if let Some(folder) = self.listing.as_ref().map(|listing| listing.path.clone()) {
            self.request(&folder);
        } else if self.is_on_start() {
            self.arrive_at_start(cx);
        }
        cx.notify();
    }
}
