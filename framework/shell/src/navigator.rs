use std::fmt;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{App, Context, EventEmitter, FocusHandle, Pixels, Point, SharedString};

use crate::location::Location;
use crate::menu::RowItem;
use crate::per_tab::TabId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavAsk {
    Go(Location),
    Replace(Location),
    Focus(FocusHandle),
    NewTab(Location),
    Reload,
    Reveal(PathBuf),
    PickFolder(FolderPick),
    FocusList,
    PopUp(PopUp),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopUp {
    pub at: Point<Pixels>,
    pub groups: Vec<Vec<RowItem>>,
}

pub type OnPicked = Rc<dyn Fn(PathBuf, &mut App)>;

#[derive(Clone)]
pub struct FolderPick {
    pub title: SharedString,
    pub accept: SharedString,
    pub picked: OnPicked,
}

impl fmt::Debug for FolderPick {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.title)
    }
}

impl PartialEq for FolderPick {
    fn eq(&self, other: &Self) -> bool {
        self.title == other.title
            && self.accept == other.accept
            && Rc::ptr_eq(&self.picked, &other.picked)
    }
}

impl Eq for FolderPick {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavEvent {
    Asked,
    Left,
    TabShown(TabId),
    TabClosed(TabId),
}

#[derive(Debug, Default)]
pub struct Navigator {
    tab: Option<TabId>,
    tabs: Vec<TabId>,
    asks: Vec<NavAsk>,
    closing: bool,
}

impl EventEmitter<NavEvent> for Navigator {}

impl Navigator {
    pub const fn tab(&self) -> Option<TabId> {
        self.tab
    }

    pub fn tabs(&self) -> &[TabId] {
        &self.tabs
    }

    pub fn go(&mut self, location: Location, cx: &mut Context<Self>) {
        self.asks.push(NavAsk::Go(location));
        cx.emit(NavEvent::Asked);
    }

    pub fn replace(&mut self, location: Location, cx: &mut Context<Self>) {
        self.asks.push(NavAsk::Replace(location));
        cx.emit(NavEvent::Asked);
    }

    pub fn focus(&mut self, handle: FocusHandle, cx: &mut Context<Self>) {
        self.asks.push(NavAsk::Focus(handle));
        cx.emit(NavEvent::Asked);
    }

    pub fn open_in_new_tab(&mut self, location: Location, cx: &mut Context<Self>) {
        self.asks.push(NavAsk::NewTab(location));
        cx.emit(NavEvent::Asked);
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.asks.push(NavAsk::Reload);
        cx.emit(NavEvent::Asked);
    }

    pub fn reveal(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.asks.push(NavAsk::Reveal(path));
        cx.emit(NavEvent::Asked);
    }

    pub fn pick_folder(
        &mut self,
        title: impl Into<SharedString>,
        accept: impl Into<SharedString>,
        picked: impl Fn(PathBuf, &mut App) + 'static,
        cx: &mut Context<Self>,
    ) {
        self.asks.push(NavAsk::PickFolder(FolderPick {
            title: title.into(),
            accept: accept.into(),
            picked: Rc::new(picked),
        }));
        cx.emit(NavEvent::Asked);
    }

    pub fn menu_at(
        &mut self,
        at: Point<Pixels>,
        groups: Vec<Vec<RowItem>>,
        cx: &mut Context<Self>,
    ) {
        self.asks.push(NavAsk::PopUp(PopUp { at, groups }));
        cx.emit(NavEvent::Asked);
    }

    pub fn focus_list(&mut self, cx: &mut Context<Self>) {
        self.asks.push(NavAsk::FocusList);
        cx.emit(NavEvent::Asked);
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.closing = true;
        cx.notify();
    }

    pub const fn closing(&self) -> bool {
        self.closing
    }

    pub fn take_asks(&mut self) -> Vec<NavAsk> {
        std::mem::take(&mut self.asks)
    }

    pub fn show(&mut self, tab: TabId, tabs: Vec<TabId>, cx: &mut Context<Self>) {
        if self.tab == Some(tab) && self.tabs == tabs {
            return;
        }
        for closed in self.tabs.iter().filter(|old| !tabs.contains(old)) {
            cx.emit(NavEvent::TabClosed(*closed));
        }
        let switched = self.tab != Some(tab);
        self.tab = Some(tab);
        self.tabs = tabs;
        if switched {
            cx.emit(NavEvent::TabShown(tab));
        }
        cx.notify();
    }
}
