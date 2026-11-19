use std::cmp::Ordering;
use std::collections::HashMap;
use std::mem;
use std::path::{Path, PathBuf};

use gpui::{ClickEvent, Context, SharedString, UniformListScrollHandle, Window};
use nova_files_api::{EntryId, Listing, ListingId, Order};

use super::MainWindow;
use super::search::Search;
use nova_files_shell::{PageRef, TabId};

use crate::assets::Icon;
use crate::jump::TypeJump;
use crate::location::Location;
use crate::places::START_LABEL;
use crate::selection::Selection;
use nova_files_places_ui as places_ui;
use nova_files_shell::History;

pub const OPEN_IN_NEW_TAB_LABEL: &str = "Open in new tab";

const LAST: usize = 1;

pub struct Tab {
    history: History<Location>,
    listing: Option<Listing>,
    selection: Option<Selection>,
    order: Order,
    shown_order: Order,
    scroll: UniformListScrollHandle,
    search: Search,
    counted: SharedString,
    listed: Option<PageRef>,
    on_its_way: Option<Location>,
}

impl Tab {
    fn place(&self) -> Option<Location> {
        self.on_its_way
            .clone()
            .or_else(|| self.history.here().cloned())
    }
}

pub struct Tabs {
    list: Vec<Option<Tab>>,
    pub(super) ids: Vec<TabId>,
    next_id: u64,
    pub(super) current: usize,
    changed: bool,
    start_if_refused: Option<ListingId>,
}

impl Default for Tabs {
    fn default() -> Self {
        Self {
            list: vec![None],
            ids: vec![TabId(0)],
            next_id: 1,
            current: 0,
            changed: false,
            start_if_refused: None,
        }
    }
}

impl MainWindow {
    pub const fn tab_count(&self) -> usize {
        self.tabs.list.len()
    }

    pub const fn current_tab(&self) -> usize {
        self.tabs.current
    }

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

    fn place_on_screen(&self) -> Option<Location> {
        self.asked
            .and_then(|_| self.requested.clone())
            .or_else(|| self.location().cloned())
    }

    pub fn new_tab(&mut self, place: &Location, cx: &mut Context<Self>) {
        if self.picker() {
            return;
        }
        let left = self.leave_tab(cx);
        self.tabs.list[self.tabs.current] = Some(left);
        self.tabs.list.push(None);
        self.tabs.ids.push(TabId(self.tabs.next_id));
        self.tabs.next_id += 1;
        self.tabs.current = self.tabs.list.len() - 1;
        match place {
            Location::Start => self.open_start(cx),
            Location::Folder(folder) => {
                self.open(folder);
                self.tabs.start_if_refused = self.asked;
            }
            Location::Page(page) => self.open_page(page.clone(), cx),
        }
        cx.notify();
    }

    pub fn open_in_new_tab(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        self.new_tab(&Location::Folder(folder), cx);
    }

    pub(crate) fn entry_aux_clicked(
        &mut self,
        entry: (ListingId, EntryId),
        event: &ClickEvent,
        cx: &mut Context<Self>,
    ) {
        if event.is_middle_click() && !self.picker() {
            self.open_entry_in_new_tab(entry, cx);
        }
    }

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

    pub fn tab_names(&self) -> Vec<SharedString> {
        self.tab_looks().into_iter().map(|(name, _)| name).collect()
    }

    pub(crate) fn tab_looks(&self) -> Vec<(SharedString, Icon)> {
        let shown = places_ui::shown(self.places(), self.pinned());
        self.tab_places()
            .iter()
            .map(|place| self.tab_look(place.as_ref(), &shown))
            .collect()
    }

    fn tab_look(
        &self,
        place: Option<&Location>,
        shown: &[places_ui::Shown],
    ) -> (SharedString, Icon) {
        match place {
            Some(Location::Start) => (START_LABEL.into(), Icon::Start),
            Some(Location::Page(page)) => (page.title(), page.icon()),
            Some(Location::Folder(folder)) => self.host.rules.named(folder).map_or_else(
                || self.folder_look(folder, shown),
                |named| (named.label, named.icon),
            ),
            None => (SharedString::default(), Icon::Folder),
        }
    }

    fn folder_look(&self, folder: &Path, shown: &[places_ui::Shown]) -> (SharedString, Icon) {
        shown
            .iter()
            .find(|shown| shown.path == *folder)
            .map_or_else(
                || (places_ui::folder_name(folder), self.known.icon_at(folder)),
                |shown| (shown.name.clone(), shown.icon),
            )
    }

    pub(crate) fn folder_name(&self, folder: &Path) -> SharedString {
        let shown = places_ui::shown(self.places(), self.pinned());
        self.tab_look(Some(&Location::Folder(folder.to_path_buf())), &shown)
            .0
    }

    pub fn switch_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index == self.tabs.current || index >= self.tabs.list.len() {
            return;
        }
        let left = self.leave_tab(cx);
        self.tabs.list[self.tabs.current] = Some(left);
        self.tabs.current = index;
        self.enter_tab(cx);
    }

    pub(super) fn reorder_tab(&mut self, from: usize, to: usize) {
        let tab = self.tabs.list.remove(from);
        self.tabs.list.insert(to, tab);
        let id = self.tabs.ids.remove(from);
        self.tabs.ids.insert(to, id);
        if let Some(current) = self.tabs.list.iter().position(Option::is_none) {
            self.tabs.current = current;
        }
    }

    pub fn next_tab(&mut self, cx: &mut Context<Self>) {
        let count = self.tab_count();
        self.switch_tab((self.tabs.current + 1) % count, cx);
    }

    pub fn previous_tab(&mut self, cx: &mut Context<Self>) {
        let count = self.tab_count();
        self.switch_tab((self.tabs.current + count - 1) % count, cx);
    }

    pub fn close_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.tab_count() <= LAST || index >= self.tab_count() {
            return;
        }
        match index.cmp(&self.tabs.current) {
            Ordering::Equal => {
                drop(self.leave_tab(cx));
                self.tabs.list.remove(index);
                self.tabs.ids.remove(index);
                self.tabs.current = index.min(self.tab_count() - 1);
                self.enter_tab(cx);
            }
            Ordering::Less => {
                self.tabs.list.remove(index);
                self.tabs.ids.remove(index);
                self.tabs.current -= 1;
                cx.notify();
            }
            Ordering::Greater => {
                self.tabs.list.remove(index);
                self.tabs.ids.remove(index);
                cx.notify();
            }
        }
    }

    pub fn close_current_tab(&mut self, cx: &mut Context<Self>) {
        self.close_tab(self.tabs.current, cx);
    }

    pub(super) fn tab_folder_refused(&mut self, id: ListingId, cx: &mut Context<Self>) -> bool {
        if self.tabs.start_if_refused != Some(id) {
            return false;
        }
        self.tabs.start_if_refused = None;
        self.open_start(cx);
        true
    }

    pub(crate) fn settle_tab_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if mem::take(&mut self.tabs.changed) {
            window.focus(&self.keys_home(cx), cx);
        }
    }

    fn leave_tab(&mut self, cx: &mut Context<Self>) -> Tab {
        self.forget_rows(cx);
        self.went_elsewhere();
        self.close_menu(cx);
        self.path_bar.update(cx, |bar, cx| {
            bar.close_menu(cx);
            bar.stop_typing(false, cx);
        });
        self.details = HashMap::new();
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
            listed: self.listed_page.take(),
            on_its_way,
        }
    }

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
        self.listed_page = tab.listed;
        self.core_moved = true;
        if let Some(place) = tab.on_its_way {
            self.request_place(&place);
        } else if self.searching() {
            self.find(cx);
        } else if let Some(page) = self.listed_page.clone() {
            self.request_listed(page);
        } else if let Some(listing) = self.listing.clone() {
            self.request(&listing.path);
            self.tabs.start_if_refused = self.asked;
        } else if self.is_on_start() {
            self.arrive_at_start(cx);
        }
        cx.notify();
    }
}
