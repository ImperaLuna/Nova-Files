use gpui::{Context, Subscription};
use nova_files_shell::{
    self as shell, IslandAsk, IslandAsked, NavAsk, NavEvent, PageRef, Scene, Shown, TabId,
};

use super::MainWindow;
use crate::island::{Confirm, NoteUndo};
use crate::location::{Builtin, Location};

impl MainWindow {
    pub(super) fn watch_slots(&self, cx: &mut Context<Self>) -> Vec<Subscription> {
        let window = cx.entity().downgrade();
        let mut watching = self.parts.observe(
            move |cx| {
                window.update(cx, |_, cx| cx.notify()).ok();
            },
            cx,
        );
        watching.extend([
            cx.observe_self(Self::publish),
            cx.subscribe(&self.host.nav, |this, _, event: &NavEvent, cx| {
                if *event == NavEvent::Asked {
                    this.take_nav_asks(cx);
                }
            }),
            cx.subscribe(&self.host.island, |this, _, _: &IslandAsked, cx| {
                this.take_island_asks(cx);
            }),
        ]);
        watching
    }

    pub fn scene(&self) -> Scene {
        Scene {
            location: self.shell_location(),
            rules: self.folder_rules(),
            selected: self.selected_len(),
            picker: self.pick.is_some(),
        }
    }

    fn selected_len(&self) -> usize {
        let shown = self.listing.as_ref().map(|listing| listing.id);
        self.selection
            .as_ref()
            .filter(|selection| Some(selection.listing) == shown)
            .map_or(0, |selection| selection.ids().len())
    }

    fn shell_location(&self) -> Option<shell::Location> {
        if let Some(folder) = self.folder() {
            return Some(shell::Location::Folder(folder.to_path_buf()));
        }
        self.location().map(Location::shell)
    }

    fn publish(&mut self, cx: &mut Context<Self>) {
        let listing = self.listing.clone();
        let selected = self
            .selection()
            .filter(|(id, _)| listing.as_ref().is_some_and(|listing| listing.id == *id))
            .map(|(_, ids)| ids)
            .unwrap_or_default();
        let shown = Shown {
            location: self.shell_location(),
            listing,
            selected,
            picker: self.pick.is_some(),
        };
        self.host
            .entries
            .update(cx, |entries, cx| entries.show(shown, cx));
        let (tab, tabs) = (self.tab_id(), self.tab_ids());
        self.host.nav.update(cx, |nav, cx| nav.show(tab, tabs, cx));
    }

    fn take_nav_asks(&mut self, cx: &mut Context<Self>) {
        let asks = self.host.nav.update(cx, |nav, _| nav.take_asks());
        for ask in asks {
            match ask {
                NavAsk::Go(place) => self.go_to_shell(place, cx),
                NavAsk::NewTab(place) => self.new_tab_at_shell(place, cx),
            }
        }
    }

    pub(crate) fn go_to_shell(&mut self, place: shell::Location, cx: &mut Context<Self>) {
        self.go_to_place(&from_shell(place), cx);
    }

    pub(crate) fn new_tab_at_shell(&mut self, place: shell::Location, cx: &mut Context<Self>) {
        self.new_tab(&from_shell(place), cx);
    }

    fn go_to_place(&mut self, place: &Location, cx: &mut Context<Self>) {
        match place {
            Location::Start => self.open_start(cx),
            Location::Folder(folder) => {
                self.open(folder);
                cx.notify();
            }
            Location::Gathered(gathering) => self.open_gathered(*gathering, cx),
            Location::Properties(paths) => self.new_tab(&Location::Properties(paths.clone()), cx),
            Location::Page(page) => self.open_page(page.clone(), cx),
        }
    }

    fn take_island_asks(&mut self, cx: &mut Context<Self>) {
        let asks = self.host.island.update(cx, |island, _| island.take_asks());
        for ask in asks {
            match ask {
                IslandAsk::Note { label, undo } => {
                    self.show_note(label, undo.map(NoteUndo::Job), cx);
                }
                IslandAsk::Ask {
                    question,
                    yes,
                    on_yes,
                } => {
                    self.island.confirm = Some(Confirm {
                        label: question,
                        yes,
                        on_yes,
                    });
                    cx.notify();
                }
            }
        }
    }

    pub fn open_page(&mut self, page: PageRef, cx: &mut Context<Self>) {
        self.history.leave();
        self.arrive_at_page(page, cx);
    }

    pub(super) fn arrive_at_page(&mut self, page: PageRef, cx: &mut Context<Self>) {
        self.let_go_of_folder(cx);
        self.stop_measuring();
        self.history.arrived(&Location::Page(page));
        cx.notify();
    }

    pub fn page_shown(&self) -> Option<&PageRef> {
        match self.location()? {
            Location::Page(page) => Some(page),
            _ => None,
        }
    }

    fn tab_id(&self) -> TabId {
        self.tabs.ids[self.tabs.current]
    }

    fn tab_ids(&self) -> Vec<TabId> {
        self.tabs.ids.clone()
    }
}

fn from_shell(place: shell::Location) -> Location {
    match place {
        shell::Location::Folder(folder) => Location::Folder(folder),
        shell::Location::Page(page) => match page.get::<Builtin>() {
            Some(Builtin::Start) => Location::Start,
            Some(Builtin::Gathered(gathering)) => Location::Gathered(*gathering),
            Some(Builtin::Properties(paths)) => Location::Properties(paths.clone()),
            None => Location::Page(page),
        },
    }
}
