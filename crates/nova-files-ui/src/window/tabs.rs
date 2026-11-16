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
//! (see `events`), so it also brings in what changed meanwhile, and the
//! sidebar's tree opens down to it. A folder still on its way when the
//! tab was left is asked for again; a search is run again and shows from
//! its top, since what it finds may have changed. A tab whose folder is
//! gone by then (deleted, or its drive unplugged) goes to Start, and so
//! does a new tab whose folder can't be read. A drive still mounting
//! when the tab is left isn't opened.
//!
//! The keys go back to the window on each change of tab, once it draws: a
//! field of the tab left (its search box, a rename) may have held them.
//!
//! A tab in the background holds its listing's names and nothing per row
//! beyond them.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::mem;
use std::path::PathBuf;

use gpui::{ClickEvent, Context, SharedString, UniformListScrollHandle, Window};
use nova_files_api::{EntryId, Listing, ListingId, Order};

use super::MainWindow;
use super::rows::release;
use super::search::Search;
use crate::assets::Icon;
use crate::drives::ShownDrive;
use crate::history::History;
use crate::jump::TypeJump;
use crate::location::Location;
use crate::places::{self, START_LABEL, TRASH_LABEL};
use crate::selection::Selection;

/// What a menu item that opens a folder in a new tab says.
pub const OPEN_IN_NEW_TAB_LABEL: &str = "Open in new tab";

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
    /// The ask that puts the tab on Start if refused: a new tab's first
    /// folder, or the folder of a tab shown again (it may be gone).
    start_if_refused: Option<ListingId>,
}

impl Default for Tabs {
    fn default() -> Self {
        Self {
            list: vec![None],
            current: 0,
            changed: false,
            start_if_refused: None,
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
        match place {
            Location::Start => self.open_start(cx),
            Location::Folder(folder) => {
                self.open(folder);
                // A folder that can't be read leaves the tab on Start.
                self.tabs.start_if_refused = self.asked;
            }
        }
        cx.notify();
    }

    /// Opens `folder` in a new tab.
    pub fn open_in_new_tab(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        self.new_tab(&Location::Folder(folder), cx);
    }

    /// A click other than the left one on entry `entry`: a middle-click
    /// on a folder opens it in a new tab, in a main window.
    pub(crate) fn entry_aux_clicked(
        &mut self,
        entry: (ListingId, EntryId),
        event: &ClickEvent,
        cx: &mut Context<Self>,
    ) {
        if event.is_middle_click() && self.pick.is_none() {
            self.open_entry_in_new_tab(entry, cx);
        }
    }

    /// Opens the folder of entry `entry` of the list on screen in a new
    /// tab, if it is a folder.
    fn open_entry_in_new_tab(
        &mut self,
        (listing, entry): (ListingId, EntryId),
        cx: &mut Context<Self>,
    ) {
        let shown = self.listing.as_ref().map(|shown| shown.id);
        if shown == Some(listing)
            && let Some(folder) = self.folder_of_entry(entry)
        {
            self.open_in_new_tab(folder, cx);
        }
    }

    /// Opens the trash in a new tab.
    pub fn open_trash_in_new_tab(&mut self, cx: &mut Context<Self>) {
        self.open_in_new_tab(self.trash.folder.clone(), cx);
    }

    /// Opens `drive` in a new tab, mounting it first when needed.
    pub(crate) fn open_drive_in_new_tab(&mut self, drive: ShownDrive, cx: &mut Context<Self>) {
        if let Some(folder) = drive.folder.clone() {
            self.open_in_new_tab(folder, cx);
        } else {
            // The new tab waits on Start until the drive is mounted.
            self.new_tab(&Location::Start, cx);
            self.open_drive(drive);
        }
    }

    /// The name of each tab in the Open list, in order.
    pub fn tab_names(&self) -> Vec<SharedString> {
        self.tab_looks().into_iter().map(|(name, _)| name).collect()
    }

    /// The name and icon of each tab in the Open list, in order.
    pub(crate) fn tab_looks(&self) -> Vec<(SharedString, Icon)> {
        let shown = places::shown(self.places(), self.pinned());
        self.tab_places()
            .iter()
            .map(|place| self.tab_look(place.as_ref(), &shown))
            .collect()
    }

    /// The name and icon of a tab at `place` in the Open list: as Places
    /// shows it when it is one of `shown`, Places' rows, else the
    /// folder's own.
    fn tab_look(&self, place: Option<&Location>, shown: &[places::Shown]) -> (SharedString, Icon) {
        match place {
            Some(Location::Start) => (START_LABEL.into(), Icon::Start),
            Some(Location::Folder(folder)) if *folder == self.trash.folder => {
                (TRASH_LABEL.into(), Icon::Trash)
            }
            Some(Location::Folder(folder)) => shown
                .iter()
                .find(|shown| shown.path == *folder)
                .map_or_else(
                    || {
                        let icon =
                            places::standard_icon_at(&self.places, folder).unwrap_or(Icon::Folder);
                        (places::folder_name(folder), icon)
                    },
                    |shown| (shown.name.clone(), shown.icon),
                ),
            None => (SharedString::default(), Icon::Folder),
        }
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
        match index.cmp(&self.tabs.current) {
            Ordering::Equal => {
                drop(self.leave_tab(cx));
                self.tabs.list.remove(index);
                self.tabs.current = index.min(self.tab_count() - 1);
                self.enter_tab(cx);
            }
            Ordering::Less => {
                self.tabs.list.remove(index);
                self.tabs.current -= 1;
                cx.notify();
            }
            Ordering::Greater => {
                self.tabs.list.remove(index);
                cx.notify();
            }
        }
    }

    /// Closes the tab on screen, unless it is the last one.
    pub fn close_current_tab(&mut self, cx: &mut Context<Self>) {
        self.close_tab(self.tabs.current, cx);
    }

    /// Listing `id` was refused. When it was a new tab's first folder or
    /// a tab's folder asked for again on coming back (deleted, or its
    /// drive gone meanwhile), the tab goes to Start rather than stay
    /// blank or show what is no longer there. Whether it did.
    pub(super) fn tab_folder_refused(&mut self, id: ListingId, cx: &mut Context<Self>) -> bool {
        if self.tabs.start_if_refused != Some(id) {
            return false;
        }
        self.tabs.start_if_refused = None;
        self.open_start(cx);
        true
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
        // A drive mounting opens in the tab that asked, or nowhere.
        self.forget_drive_to_open();
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
        } else if let Some(listing) = self.listing.clone() {
            // The sidebar's tree comes along to the tab's folder.
            self.tree_follows(&listing, true);
            self.request(&listing.path);
            self.tabs.start_if_refused = self.asked;
        } else if self.is_on_start() {
            self.arrive_at_start(cx);
        }
        cx.notify();
    }
}
