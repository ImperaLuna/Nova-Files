#![forbid(unsafe_code)]

mod access;
mod page;
mod view;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{
    AnyElement, App, Context, Pixels, Point, SharedString, Subscription, WeakEntity, Window,
};
use nova_files_api::{AccessChange, Event as CoreEvent, JobId, ModeChange, Operation, Reach};
use nova_files_open_api::{self as open_api, OpenWith, OpenWithAnswer, OpenWithId};
use nova_files_properties_api::{
    Command, Event, MeasureId, Properties, PropertiesId, ScanState, Size,
};
use nova_files_shell::assets::Icon;
use nova_files_shell::format::{self, ITEM, ITEMS};
use nova_files_shell::paths::name_of;
use nova_files_shell::{Host, Location, Look, NavEvent, PageKind, Part, RowItem, Slots, TabId};

pub use access::{Level as AccessLevel, Who, runs, shared_level};
pub use page::{Measure, Page};
pub use view::TopFacts;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertiesPage {
    paths: Vec<PathBuf>,
}

impl PropertiesPage {
    pub const fn new(paths: Vec<PathBuf>) -> Self {
        Self { paths }
    }

    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    pub fn folder(&self) -> Option<&Path> {
        let folder = self.paths.first()?.parent()?;
        self.paths
            .iter()
            .all(|path| path.parent() == Some(folder))
            .then_some(folder)
    }
}

impl PageKind for PropertiesPage {
    fn title(&self) -> SharedString {
        match &self.paths[..] {
            [one] => name_of(one).into(),
            many => format::count(many.len(), ITEM, ITEMS).into(),
        }
    }

    fn icon(&self) -> Icon {
        Icon::Info
    }
}

pub struct PropertiesPart {
    host: Host,
    this: WeakEntity<Self>,
    pages: HashMap<TabId, Page>,
    shown: Option<(TabId, PropertiesPage)>,
    access_jobs: HashSet<JobId>,
    _tabs: Subscription,
}

impl PropertiesPart {
    pub fn page(&self) -> Option<&Page> {
        let (tab, _) = self.shown.as_ref()?;
        self.pages.get(tab)
    }

    fn page_mut(&mut self) -> Option<&mut Page> {
        let (tab, _) = self.shown.as_ref()?;
        self.pages.get_mut(tab)
    }

    pub fn shown_page(&self) -> Option<&PropertiesPage> {
        self.shown.as_ref().map(|(_, page)| page)
    }

    pub fn measured(&self) -> Option<Measure> {
        self.page()?.measured()
    }

    pub fn apps_to_open_with(&self) -> &[open_api::App] {
        self.page().map_or(&[], Page::apps)
    }

    pub fn default_app(&self) -> Option<&open_api::App> {
        self.page()?.default_app()
    }

    pub fn may_change_access(&self) -> bool {
        self.page().is_some_and(Page::may_change_access)
    }

    pub fn reaches_inside(&self) -> bool {
        self.page().is_some_and(Page::reaches_inside)
    }

    pub fn toggle_inside(&mut self, cx: &mut Context<Self>) {
        if let Some(page) = self.page_mut() {
            page.inside = !page.inside;
            cx.notify();
        }
    }

    fn shown_turn(&mut self, page: &PropertiesPage, tab: TabId, cx: &mut Context<Self>) {
        let fresh = self
            .pages
            .get(&tab)
            .is_none_or(|kept| !kept.same_paths(page.paths()));
        if fresh {
            self.pages.insert(tab, Page::new(page.paths()));
        }
        self.shown = Some((tab, page.clone()));
        self.ask_properties();
        self.measure();
        self.ask_apps();
        cx.notify();
    }

    fn left_turn(&mut self, _: &PropertiesPage, tab: TabId, _: &mut Context<Self>) {
        if let Some(page) = self.pages.get_mut(&tab) {
            stop_measuring(&self.host, page);
        }
        if self.shown.as_ref().is_some_and(|(shown, _)| *shown == tab) {
            self.shown = None;
        }
    }

    fn ask_properties(&mut self) {
        let (host, Some(page)) = (self.host.clone(), self.page_mut()) else {
            return;
        };
        page.asked.clear();
        for (index, item) in page.items.iter().enumerate() {
            let id = PropertiesId(host.asks.next());
            let path = item.path.clone();
            if host.link.send(Command::Properties { id, path }).is_ok() {
                page.asked.insert(id, index);
            }
        }
    }

    fn measure(&mut self) {
        let (host, Some(page)) = (self.host.clone(), self.page_mut()) else {
            return;
        };
        let id = MeasureId(host.asks.next());
        let before = page
            .measure
            .filter(|measure| measure.state == ScanState::Done)
            .map(|measure| measure.size);
        let paths = page.paths();
        if paths.is_empty() {
            return;
        }
        let sent = host.link.send(Command::Measure { id, paths }).is_ok();
        page.measure = sent.then_some(Measure {
            id,
            size: Size::default(),
            state: ScanState::Counting,
            before,
        });
    }

    fn ask_apps(&mut self) {
        let (host, Some(page)) = (self.host.clone(), self.page_mut()) else {
            return;
        };
        let id = OpenWithId(host.asks.next());
        let paths = page.paths();
        if paths.len() != 1 {
            return;
        }
        let sent = host
            .link
            .send(open_api::Command::OpenWith(OpenWith::Apps { id, paths }))
            .is_ok();
        page.opens_with.asked = sent.then_some(id);
    }

    fn arrived(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Properties { id, properties } => {
                self.properties_arrived(id, Some(properties), cx);
            }
            Event::PropertiesUnreadable { id, .. } => self.properties_arrived(id, None, cx),
            Event::SizeMeasured { id, size, state } => {
                let Some(measure) = self
                    .page_mut()
                    .and_then(|page| page.measure.as_mut())
                    .filter(|measure| measure.id == id)
                else {
                    return;
                };
                measure.size = size;
                measure.state = state;
                cx.notify();
            }
            Event::SpaceMeasured { .. } | Event::SpaceUnreadable { .. } => {}
        }
    }

    fn properties_arrived(
        &mut self,
        id: PropertiesId,
        read: Option<Arc<Properties>>,
        cx: &mut Context<Self>,
    ) {
        let Some(page) = self.page_mut() else {
            return;
        };
        let Some(item) = page
            .asked
            .remove(&id)
            .and_then(|index| page.items.get_mut(index))
        else {
            return;
        };
        let gone = read.is_none();
        if gone != item.gone {
            if gone {
                page.gone += 1;
            } else {
                page.gone -= 1;
            }
        }
        item.gone = gone;
        item.read = read;
        if page.gone == page.items.len() {
            self.host
                .nav
                .update(cx, |nav, cx| nav.go(Location::start(), cx));
        }
        cx.notify();
    }

    fn open_with_arrived(&mut self, event: open_api::Event, cx: &mut Context<Self>) {
        let open_api::Event::OpenWith(OpenWithAnswer::Apps { id, mime, apps }) = event else {
            return;
        };
        let Some(opens_with) = self
            .page_mut()
            .map(|page| &mut page.opens_with)
            .filter(|opens_with| opens_with.asked == Some(id))
        else {
            return;
        };
        opens_with.asked = None;
        opens_with.mime = mime;
        opens_with.apps = apps;
        cx.notify();
    }

    fn watch(&mut self, event: &CoreEvent, _: &mut Context<Self>) {
        match event {
            CoreEvent::JobStarted { job, operation, .. }
            | CoreEvent::JobRunning { job, operation, .. }
                if matches!(operation, Operation::ChangeAccess { .. }) =>
            {
                self.access_jobs.insert(*job);
            }
            CoreEvent::JobFinished { job, .. } if self.access_jobs.remove(job) => {
                self.ask_properties();
            }
            _ => {}
        }
    }

    pub(crate) fn open_apps_menu(&self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(opens_with) = self.page().map(|page| &page.opens_with) else {
            return;
        };
        let Some(mime) = opens_with.mime.clone() else {
            return;
        };
        let items = opens_with
            .apps
            .iter()
            .map(|app| {
                let (mime, id) = (mime.clone(), app.id.clone());
                let part = self.this.clone();
                let run = move |cx: &mut App| {
                    let (mime, id) = (mime.clone(), id.clone());
                    part.update(cx, |part, _| part.set_default_app(mime, id))
                        .ok();
                };
                if app.default {
                    RowItem::new(Icon::Check, app.name.clone(), run).with_look(Look::On)
                } else {
                    RowItem::new(Icon::Open, app.name.clone(), run)
                }
            })
            .collect();
        self.host
            .nav
            .update(cx, |nav, cx| nav.menu_at(at, vec![items], cx));
    }

    fn set_default_app(&mut self, mime: String, app: String) {
        self.host
            .link
            .send(open_api::Command::OpenWith(OpenWith::SetDefault {
                mime,
                app,
            }))
            .ok();
        self.ask_apps();
    }

    pub(crate) fn open_access_menu(&self, who: Who, at: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.may_change_access() {
            return;
        }
        let shown = self.page().and_then(|page| shared_level(&page.read(), who));
        let items = AccessLevel::ALL
            .into_iter()
            .map(|level| {
                let part = self.this.clone();
                let run = move |cx: &mut App| {
                    part.update(cx, |part, cx| part.set_access(who, level, cx))
                        .ok();
                };
                if shown == Some(level) {
                    RowItem::new(Icon::Check, level.label(), run).with_look(Look::On)
                } else {
                    RowItem::new(level.icon(), level.label(), run)
                }
            })
            .collect();
        self.host
            .nav
            .update(cx, |nav, cx| nav.menu_at(at, vec![items], cx));
    }

    fn set_access(&self, who: Who, level: AccessLevel, cx: &mut Context<Self>) {
        let (files, folders) = who.change_to(level);
        self.change_access(files, folders, cx);
    }

    pub fn toggle_runs(&mut self, cx: &mut Context<Self>) {
        let Some(mode) = self
            .page()
            .and_then(|page| page.read().first().map(|one| one.mode))
        else {
            return;
        };
        self.change_access(
            access::run_change(!runs(mode), mode),
            ModeChange::default(),
            cx,
        );
    }

    fn change_access(&self, files: ModeChange, folders: ModeChange, cx: &mut Context<Self>) {
        if !self.may_change_access() {
            return;
        }
        let Some(page) = self.page() else {
            return;
        };
        let reach = if page.inside {
            Reach::Inside
        } else {
            Reach::Chosen
        };
        let operation = Operation::ChangeAccess {
            paths: page.paths(),
            change: AccessChange {
                owner: None,
                group: None,
                files,
                folders,
                reach,
            },
        };
        self.host
            .island
            .update(cx, |island, cx| island.start_job(operation, cx));
    }
}

fn stop_measuring(host: &Host, page: &mut Page) {
    let Some(measure) = page
        .measure
        .take_if(|measure| measure.state == ScanState::Counting)
    else {
        return;
    };
    host.link.send(Command::StopMeasure(measure.id)).ok();
}

impl Part for PropertiesPart {
    fn open(host: &Host, cx: &mut Context<Self>) -> Self {
        let tabs = cx.subscribe(&host.nav, |part: &mut Self, _, event: &NavEvent, _| {
            if let NavEvent::TabClosed(tab) = event {
                part.pages.remove(tab);
            }
        });
        Self {
            host: host.clone(),
            this: cx.entity().downgrade(),
            pages: HashMap::new(),
            shown: None,
            access_jobs: HashSet::new(),
            _tabs: tabs,
        }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.on_event(Self::arrived);
        slots.on_event(Self::open_with_arrived);
        slots.sends::<Command>();
        slots.sends::<open_api::Command>();
        slots.watch(Self::watch);
        slots.page_shown::<PropertiesPage>(Self::shown_turn);
        slots.page_left::<PropertiesPage>(Self::left_turn);
        slots.page::<PropertiesPage>(draw);
    }
}

fn draw(
    part: &mut PropertiesPart,
    _: &PropertiesPage,
    _: &mut Window,
    cx: &mut Context<PropertiesPart>,
) -> AnyElement {
    view::page(part, cx)
}
