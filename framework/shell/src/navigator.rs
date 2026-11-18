use gpui::{Context, EventEmitter};

use crate::location::Location;
use crate::per_tab::TabId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavAsk {
    Go(Location),
    NewTab(Location),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavEvent {
    Asked,
    TabClosed(TabId),
}

#[derive(Debug, Default)]
pub struct Navigator {
    tab: Option<TabId>,
    tabs: Vec<TabId>,
    asks: Vec<NavAsk>,
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

    pub fn open_in_new_tab(&mut self, location: Location, cx: &mut Context<Self>) {
        self.asks.push(NavAsk::NewTab(location));
        cx.emit(NavEvent::Asked);
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
        self.tab = Some(tab);
        self.tabs = tabs;
        cx.notify();
    }
}
