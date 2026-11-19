use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{App, Context, Entity, Subscription};
use nova_files_api::{HiddenFiles, ListingId};
use nova_files_picker_api::PickId;
use nova_files_shell::path_bar::PathBarEvent;
use nova_files_shell::{
    self as shell, EntriesAsked, FolderName, Host, IslandAsk, IslandAsked, NavAsk, NavEvent,
    Opening, PageRef, Part, Scene, Shown, TabId,
};
use nova_files_shell::{KnownFolders, Published, ThumbnailsShown};

use super::search::FocusTo;
use super::{MainWindow, Went};
use crate::island::{Confirm, NoteUndo};
use crate::location::{Builtin, Location};
use crate::selection::Selection;

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
            cx.observe(&self.host.nav, |_, _, cx| cx.notify()),
            cx.observe(&self.path_bar, |_, _, cx| cx.notify()),
            cx.subscribe(&self.path_bar, |this, _, event: &PathBarEvent, cx| {
                this.path_bar_said(event, cx);
            }),
            cx.subscribe(&self.host.nav, |this, _, event: &NavEvent, cx| {
                if *event == NavEvent::Asked {
                    this.take_nav_asks(cx);
                }
            }),
            cx.subscribe(&self.host.island, |this, _, _: &IslandAsked, cx| {
                this.take_island_asks(cx);
            }),
            cx.subscribe(&self.host.entries, |this, _, _: &EntriesAsked, cx| {
                this.take_entries_asks(cx);
            }),
            cx.on_release(|this, cx| this.parts.closing(cx)),
            cx.observe_global::<Published<HiddenFiles>>(Self::hidden_published),
            cx.observe_global::<Published<ThumbnailsShown>>(Self::thumbnails_published),
            cx.observe_global::<Published<KnownFolders>>(Self::known_published),
        ]);
        watching
    }

    fn path_bar_said(&mut self, event: &PathBarEvent, cx: &mut Context<Self>) {
        match event {
            PathBarEvent::Typed(target) => {
                let before = self.asked;
                self.open(target);
                let asked = self.asked.filter(|_| self.asked != before);
                self.path_bar.update(cx, |bar, cx| bar.going(asked, cx));
            }
            PathBarEvent::Left => self.focus_later(FocusTo::List, cx),
        }
    }

    pub const fn host(&self) -> &Host {
        &self.host
    }

    pub fn named_here(&self) -> Option<FolderName> {
        self.host.rules.named(self.folder()?)
    }

    pub fn part<P: Part>(&self) -> Option<Entity<P>> {
        self.parts.part::<P>()
    }

    pub fn scene(&self) -> Scene {
        Scene {
            location: self.shell_location(),
            rules: self.folder_rules(),
            selected: self.selected_len(),
            picker: self.host.opening.is_some(),
            searching: self.searching(),
            gathered: self.gathered_here().is_some(),
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

    pub(crate) fn open_as_picker(&mut self, id: PickId) {
        if !self.picker() {
            self.host.opening = Opening::of(id);
        }
    }

    pub const fn picker(&self) -> bool {
        self.host.opening.is_some()
    }

    pub(crate) fn closing(&self, cx: &App) -> bool {
        self.host.nav.read(cx).closing()
    }

    pub fn dims(&self, index: usize, cx: &App) -> bool {
        let entry = self
            .listing
            .as_ref()
            .and_then(|listing| listing.entries.get(index));
        self.pick_dims(index) || entry.is_some_and(|entry| self.parts.dims(entry, cx))
    }

    pub(super) fn tell_parts_listed(&mut self, reordered: bool, cx: &mut Context<Self>) {
        self.publish(cx);
        if let Some(listing) = self.listing.clone() {
            self.parts.listed(&listing, reordered, cx);
        }
    }

    fn take_entries_asks(&mut self, cx: &mut Context<Self>) {
        let (now, later) = self.host.entries.update(cx, |entries, _| {
            (entries.take_select(), entries.take_select_later())
        });
        if let Some((folder, names)) = later {
            self.select_when_listed(folder, names, cx);
        }
        let Some(names) = now else {
            return;
        };
        let Some(listing) = &self.listing else {
            return;
        };
        let names: HashSet<&OsStr> = names.iter().map(OsString::as_os_str).collect();
        let (id, entries) = (listing.id, Arc::clone(&listing.entries));
        self.reselect(|_, _| Selection::named_all(id, &entries, &names), cx);
    }

    fn select_when_listed(
        &mut self,
        folder: PathBuf,
        names: Vec<OsString>,
        cx: &mut Context<Self>,
    ) {
        let listed = self
            .listing
            .as_ref()
            .filter(|listing| listing.path == folder);
        if let Some(listing) = listed {
            let wanted: HashSet<&OsStr> = names.iter().map(OsString::as_os_str).collect();
            if let Some(selection) = Selection::named_all(listing.id, &listing.entries, &wanted) {
                self.reselect(|_, _| Some(selection), cx);
                return;
            }
        }
        self.select_on_arrival(folder, names);
    }

    pub(crate) fn publish(&mut self, cx: &mut Context<Self>) {
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
            picker: self.host.opening.is_some(),
            asked: self.asked,
            searching: self.searching(),
            gathered: self.gathered_here().is_some(),
        };
        self.host
            .entries
            .update(cx, |entries, cx| entries.show(shown, cx));
        let (tab, tabs) = (self.tab_id(), self.tab_ids());
        let left = std::mem::take(&mut self.went) == Went::Elsewhere;
        self.host.nav.update(cx, |nav, cx| {
            nav.show(tab, tabs, cx);
            if left {
                cx.emit(NavEvent::Left);
            }
        });
    }

    fn take_nav_asks(&mut self, cx: &mut Context<Self>) {
        let asks = self.host.nav.update(cx, |nav, _| nav.take_asks());
        let went = std::mem::take(&mut self.went);
        for ask in asks {
            match ask {
                NavAsk::Go(place) => self.go_to_shell(place, cx),
                NavAsk::NewTab(place) => self.new_tab_at_shell(place, cx),
                NavAsk::Reload => self.reload(),
                NavAsk::Reveal(path) => self.show_in_folder(&path),
                NavAsk::FocusList => self.focus_later(FocusTo::List, cx),
                NavAsk::PopUp(pop_up) => self.open_part_menu(pop_up.at, pop_up.groups, cx),
                NavAsk::PickFolder(pick) => {
                    self.pick_folder(&pick.title, &pick.accept, cx, move |_, path, cx| {
                        let picked = pick.picked.clone();
                        cx.defer(move |cx| picked(path, cx));
                    });
                }
            }
        }
        self.went = went;
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
                IslandAsk::StartJob(operation) => {
                    self.start_job(operation);
                }
                IslandAsk::Note { id, label, undo } => {
                    self.show_note(label, undo.map(NoteUndo::from), cx);
                    self.island.note_owner = Some(id);
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
        if self.parts.lists(&page) {
            self.forget_drive_to_open();
            self.request_listed(page);
            cx.notify();
        } else {
            self.arrive_at_page(page, cx);
        }
    }

    pub(super) fn request_listed(&mut self, page: PageRef) -> bool {
        let id = ListingId(self.next_listing);
        let command = self.parts.list_page(&page, id, self.order);
        command.is_some() && self.send_listing(Location::Page(page), id, command)
    }

    pub const fn listed_page(&self) -> Option<&PageRef> {
        self.listed_page.as_ref()
    }

    pub(super) fn arrive_at_page(&mut self, page: PageRef, cx: &mut Context<Self>) {
        self.let_go_of_folder(cx);
        self.stop_measuring();
        self.history.arrived(&Location::Page(page));
        cx.notify();
    }

    pub fn page_shown(&self) -> Option<&PageRef> {
        match self.location()? {
            Location::Page(page) if !self.parts.lists(page) => Some(page),
            _ => None,
        }
    }

    fn show_in_folder(&mut self, path: &Path) {
        let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
            return;
        };
        self.select_on_arrival(folder.to_path_buf(), vec![name.to_os_string()]);
        self.open(folder);
    }

    fn reload(&mut self) {
        if let Some(folder) = self.folder().map(Path::to_path_buf)
            && !self.waiting()
        {
            self.request(&folder);
        }
    }

    fn tab_id(&self) -> TabId {
        self.tabs.ids[self.tabs.current]
    }

    fn tab_ids(&self) -> Vec<TabId> {
        self.tabs.ids.clone()
    }
}

pub fn from_shell(place: shell::Location) -> Location {
    match place {
        shell::Location::Folder(folder) => Location::Folder(folder),
        shell::Location::Page(page) if page.get::<shell::StartPage>().is_some() => Location::Start,
        shell::Location::Page(page) => match page.get::<Builtin>() {
            Some(Builtin::Gathered(gathering)) => Location::Gathered(*gathering),
            Some(Builtin::Properties(paths)) => Location::Properties(paths.clone()),
            None => Location::Page(page),
        },
    }
}
