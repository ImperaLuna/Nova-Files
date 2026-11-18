use std::any::{Any, TypeId};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{
    AnyElement, AnyView, App, AppContext as _, Div, Entity, Global, KeyBinding, SharedString,
    Subscription,
};
use nova_files_api::{Entry, Event, Listing};

use crate::assets::Icon;
use nova_files_messages::{Envelope, MessageType};

use crate::command::CommandSpec;
use crate::host::Host;
use crate::location::{Location, PageRef};
use crate::menu::{MenuAt, MenuItem, SidebarAt, SidebarRow};
use crate::part::{Card, Part, Slots};
use crate::scene::Scene;
use crate::slot_view::{Draw, SlotView};

type ShownCell = Box<dyn Fn(&Entry, &App) -> Option<SharedString>>;

pub struct ShownColumn {
    pub head: SharedString,
    pub selector: &'static str,
    cell: ShownCell,
}

impl ShownColumn {
    pub fn cell(&self, entry: &Entry, cx: &App) -> Option<SharedString> {
        (self.cell)(entry, cx)
    }
}

trait OpenPart {
    fn deliver(&self, envelope: Envelope, cx: &mut App) -> Result<(), Envelope>;
    fn bind(&self, root: Div) -> Div;
    fn applies(&self, spec: &CommandSpec, scene: &Scene, cx: &App) -> Option<bool>;
    fn menu(&self, at: MenuAt, scene: &Scene, cx: &App) -> Vec<MenuItem>;
    fn page(&self, page: &PageRef) -> Option<AnyView>;
    fn column(&self, scene: &Scene, cx: &App) -> Option<ShownColumn>;
    fn sidebar(&self, at: SidebarAt, scene: &Scene, cx: &App) -> Vec<SidebarRow>;
    fn start_cards(&self) -> Vec<AnyView>;
    fn bars(&self) -> Vec<AnyView>;
    fn overlays(&self) -> Vec<AnyView>;
    fn list_view(&self, scene: &Scene, cx: &App) -> Option<AnyView>;
    fn row_icon(&self, entry: &Entry, cx: &App) -> Option<AnyElement>;
    fn open_entry(&self, entry: &Entry, path: &Path, cx: &mut App) -> bool;
    fn observe(&self, changed: Rc<dyn Fn(&mut App)>, cx: &mut App) -> Subscription;
    fn watch(&self, event: &Event, cx: &mut App);
    fn empty_note(&self, scene: &Scene, cx: &App) -> Option<(Icon, SharedString)>;
    fn name_cell(&self, entry: &Entry, cx: &App) -> Option<AnyElement>;
    fn listed(&self, listing: &Listing, cx: &mut App);
    fn takes_drops(&self, page: &PageRef) -> bool;
    fn drop_on(
        &self,
        page: &PageRef,
        paths: Vec<PathBuf>,
        cx: &mut App,
    ) -> Result<(), Vec<PathBuf>>;
    fn as_any(&self) -> &dyn Any;
}

struct Opened<P: Part> {
    entity: Entity<P>,
    host: Host,
    slots: Slots<P>,
    pages: Vec<(TypeId, AnyView)>,
    cards: Vec<AnyView>,
    bars: Vec<AnyView>,
    overlays: Vec<AnyView>,
    list_views: Vec<AnyView>,
}

impl<P: Part> OpenPart for Opened<P> {
    fn deliver(&self, envelope: Envelope, cx: &mut App) -> Result<(), Envelope> {
        let Some(slot) = self.slots.events.get(&envelope.kind()) else {
            return Err(envelope);
        };
        self.entity.update(cx, |part, cx| slot(part, envelope, cx));
        Ok(())
    }

    fn bind(&self, root: Div) -> Div {
        self.slots.commands.iter().fold(root, |root, command| {
            (command.bind)(root, &self.entity, &self.host)
        })
    }

    fn applies(&self, spec: &CommandSpec, scene: &Scene, cx: &App) -> Option<bool> {
        let command = self
            .slots
            .commands
            .iter()
            .find(|command| command.spec == spec)?;
        Some((command.applies)(self.entity.read(cx), scene, cx))
    }

    fn menu(&self, at: MenuAt, scene: &Scene, cx: &App) -> Vec<MenuItem> {
        let part = self.entity.read(cx);
        self.slots
            .menus
            .iter()
            .filter(|(place, _)| *place == at)
            .flat_map(|(_, items)| items(part, scene, cx))
            .collect()
    }

    fn page(&self, page: &PageRef) -> Option<AnyView> {
        self.pages
            .iter()
            .find(|(kind, _)| *kind == page.kind())
            .map(|(_, view)| view.clone())
    }

    fn column(&self, scene: &Scene, cx: &App) -> Option<ShownColumn> {
        let part = self.entity.read(cx);
        let column = self
            .slots
            .columns
            .iter()
            .find_map(|column| column(part, scene, cx))?;
        let entity = self.entity.clone();
        let cell = column.cell;
        Some(ShownColumn {
            head: column.head,
            selector: column.selector,
            cell: Box::new(move |entry, cx| cell(entity.read(cx), entry, cx)),
        })
    }

    fn sidebar(&self, at: SidebarAt, scene: &Scene, cx: &App) -> Vec<SidebarRow> {
        let part = self.entity.read(cx);
        self.slots
            .sidebar
            .iter()
            .filter(|(place, _)| *place == at)
            .flat_map(|(_, rows)| rows(part, scene, cx))
            .collect()
    }

    fn start_cards(&self) -> Vec<AnyView> {
        self.cards.clone()
    }

    fn bars(&self) -> Vec<AnyView> {
        self.bars.clone()
    }

    fn overlays(&self) -> Vec<AnyView> {
        self.overlays.clone()
    }

    fn list_view(&self, scene: &Scene, cx: &App) -> Option<AnyView> {
        let part = self.entity.read(cx);
        self.slots
            .list_views
            .iter()
            .zip(&self.list_views)
            .find(|((applies, _), _)| applies(part, scene, cx))
            .map(|(_, view)| view.clone())
    }

    fn row_icon(&self, entry: &Entry, cx: &App) -> Option<AnyElement> {
        let part = self.entity.read(cx);
        self.slots
            .row_icons
            .iter()
            .find_map(|icon| icon(part, entry, cx))
    }

    fn open_entry(&self, entry: &Entry, path: &Path, cx: &mut App) -> bool {
        if self.slots.opens.is_empty() {
            return false;
        }
        self.entity.update(cx, |part, cx| {
            self.slots
                .opens
                .iter()
                .any(|opens| opens(part, entry, path, cx))
        })
    }

    fn observe(&self, changed: Rc<dyn Fn(&mut App)>, cx: &mut App) -> Subscription {
        cx.observe(&self.entity, move |_, cx| changed(cx))
    }

    fn watch(&self, event: &Event, cx: &mut App) {
        if self.slots.watches.is_empty() {
            return;
        }
        self.entity.update(cx, |part, cx| {
            for watch in &self.slots.watches {
                watch(part, event, cx);
            }
        });
    }

    fn empty_note(&self, scene: &Scene, cx: &App) -> Option<(Icon, SharedString)> {
        let part = self.entity.read(cx);
        self.slots
            .empty_notes
            .iter()
            .find_map(|note| note(part, scene, cx))
    }

    fn name_cell(&self, entry: &Entry, cx: &App) -> Option<AnyElement> {
        let part = self.entity.read(cx);
        self.slots
            .name_cells
            .iter()
            .find_map(|cell| cell(part, entry, cx))
    }

    fn listed(&self, listing: &Listing, cx: &mut App) {
        if self.slots.listed.is_empty() {
            return;
        }
        self.entity.update(cx, |part, cx| {
            for listed in &self.slots.listed {
                listed(part, listing, cx);
            }
        });
    }

    fn takes_drops(&self, page: &PageRef) -> bool {
        self.slots
            .drops
            .iter()
            .any(|(kind, _)| *kind == page.kind())
    }

    fn drop_on(
        &self,
        page: &PageRef,
        paths: Vec<PathBuf>,
        cx: &mut App,
    ) -> Result<(), Vec<PathBuf>> {
        let Some((_, slot)) = self
            .slots
            .drops
            .iter()
            .find(|(kind, _)| *kind == page.kind())
        else {
            return Err(paths);
        };
        let slot = Rc::clone(slot);
        self.entity
            .update(cx, |part, cx| slot(part, page, paths, cx));
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Clone, Copy)]
struct Kind {
    open: fn(&Host, &mut App) -> Box<dyn OpenPart>,
    takes: fn() -> Vec<MessageType>,
    sends: fn() -> Vec<MessageType>,
    specs: fn() -> Vec<&'static CommandSpec>,
}

#[derive(Clone, Default)]
pub struct Parts {
    kinds: Vec<Kind>,
}

impl Global for Parts {}

impl Parts {
    pub fn add<P: Part>(&mut self) {
        self.kinds.push(Kind {
            open: open::<P>,
            takes: takes::<P>,
            sends: sends::<P>,
            specs: specs::<P>,
        });
    }

    pub fn takes(&self) -> Vec<MessageType> {
        self.kinds.iter().flat_map(|kind| (kind.takes)()).collect()
    }

    pub fn sends(&self) -> Vec<MessageType> {
        self.kinds.iter().flat_map(|kind| (kind.sends)()).collect()
    }

    pub fn bindings(&self) -> Vec<KeyBinding> {
        self.kinds
            .iter()
            .flat_map(|kind| (kind.specs)())
            .flat_map(CommandSpec::bindings)
            .collect()
    }

    pub fn open(&self, host: &Host, cx: &mut App) -> WindowParts {
        WindowParts {
            opened: self
                .kinds
                .iter()
                .map(|kind| (kind.open)(host, cx))
                .collect(),
        }
    }
}

fn slots_of<P: Part>() -> Slots<P> {
    let mut slots = Slots::default();
    P::slots(&mut slots);
    slots
}

fn open<P: Part>(host: &Host, cx: &mut App) -> Box<dyn OpenPart> {
    let entity = cx.new(|cx| P::open(host, cx));
    let slots = slots_of::<P>();
    let pages = slots
        .pages
        .iter()
        .map(|(kind, render)| {
            let (part, host, render) = (entity.clone(), host.clone(), Rc::clone(render));
            let draw: Draw = Box::new(move |window, cx| {
                let page = match host.entries.read(cx).location() {
                    Some(Location::Page(page)) => page.clone(),
                    _ => return None,
                };
                part.update(cx, |part, cx| render(part, &page, window, cx))
            });
            (*kind, slot_view(&entity, draw, true, cx))
        })
        .collect();
    let cards = card_views(&entity, host, &slots.start_cards, false, cx);
    let bars = card_views(&entity, host, &slots.bars, false, cx);
    let overlays = card_views(&entity, host, &slots.overlays, false, cx);
    let views: Vec<Card<P>> = slots.list_views.iter().map(|(_, draw)| *draw).collect();
    let list_views = card_views(&entity, host, &views, true, cx);
    Box::new(Opened {
        entity,
        host: host.clone(),
        slots,
        pages,
        cards,
        bars,
        overlays,
        list_views,
    })
}

fn card_views<P: Part>(
    entity: &Entity<P>,
    host: &Host,
    cards: &[Card<P>],
    fills: bool,
    cx: &mut App,
) -> Vec<AnyView> {
    cards
        .iter()
        .map(|&card| {
            let (part, host) = (entity.clone(), host.clone());
            let draw: Draw = Box::new(move |window, cx| {
                let scene = host.scene(cx);
                part.update(cx, |part, cx| card(part, &scene, window, cx))
            });
            slot_view(entity, draw, fills, cx)
        })
        .collect()
}

fn slot_view<P: Part>(part: &Entity<P>, draw: Draw, fills: bool, cx: &mut App) -> AnyView {
    let part = part.clone();
    cx.new(|cx| {
        cx.observe(&part, |_, _, cx| cx.notify()).detach();
        SlotView::new(draw, fills)
    })
    .into()
}

fn takes<P: Part>() -> Vec<MessageType> {
    slots_of::<P>().takes()
}

fn sends<P: Part>() -> Vec<MessageType> {
    slots_of::<P>().sent()
}

fn specs<P: Part>() -> Vec<&'static CommandSpec> {
    slots_of::<P>().specs()
}

#[derive(Default)]
pub struct WindowParts {
    opened: Vec<Box<dyn OpenPart>>,
}

impl WindowParts {
    pub fn deliver(&self, mut envelope: Envelope, cx: &mut App) -> Result<(), Envelope> {
        for part in &self.opened {
            match part.deliver(envelope, cx) {
                Ok(()) => return Ok(()),
                Err(back) => envelope = back,
            }
        }
        Err(envelope)
    }

    pub fn bind(&self, root: Div) -> Div {
        self.opened.iter().fold(root, |root, part| part.bind(root))
    }

    pub fn applies(&self, spec: &CommandSpec, scene: &Scene, cx: &App) -> bool {
        self.opened
            .iter()
            .find_map(|part| part.applies(spec, scene, cx))
            .unwrap_or(true)
    }

    pub fn menu(&self, at: MenuAt, scene: &Scene, cx: &App) -> Vec<Vec<MenuItem>> {
        self.opened
            .iter()
            .map(|part| {
                part.menu(at, scene, cx)
                    .into_iter()
                    .filter(|item| self.applies(item.run, scene, cx))
                    .collect::<Vec<_>>()
            })
            .filter(|items| !items.is_empty())
            .collect()
    }

    pub fn page(&self, page: &PageRef) -> Option<AnyView> {
        self.opened.iter().find_map(|part| part.page(page))
    }

    pub fn column(&self, scene: &Scene, cx: &App) -> Option<ShownColumn> {
        self.opened.iter().find_map(|part| part.column(scene, cx))
    }

    pub fn sidebar(&self, at: SidebarAt, scene: &Scene, cx: &App) -> Vec<SidebarRow> {
        self.opened
            .iter()
            .flat_map(|part| part.sidebar(at, scene, cx))
            .collect()
    }

    pub fn start_cards(&self) -> Vec<AnyView> {
        self.opened
            .iter()
            .flat_map(|part| part.start_cards())
            .collect()
    }

    pub fn watch(&self, event: &Event, cx: &mut App) {
        for part in &self.opened {
            part.watch(event, cx);
        }
    }

    pub fn empty_note(&self, scene: &Scene, cx: &App) -> Option<(Icon, SharedString)> {
        self.opened
            .iter()
            .find_map(|part| part.empty_note(scene, cx))
    }

    pub fn name_cell(&self, entry: &Entry, cx: &App) -> Option<AnyElement> {
        self.opened
            .iter()
            .find_map(|part| part.name_cell(entry, cx))
    }

    pub fn listed(&self, listing: &Listing, cx: &mut App) {
        for part in &self.opened {
            part.listed(listing, cx);
        }
    }

    pub fn takes_drops(&self, page: &PageRef) -> bool {
        self.opened.iter().any(|part| part.takes_drops(page))
    }

    pub fn drop_on(&self, page: &PageRef, mut paths: Vec<PathBuf>, cx: &mut App) {
        for part in &self.opened {
            match part.drop_on(page, paths, cx) {
                Ok(()) => return,
                Err(back) => paths = back,
            }
        }
    }

    pub fn bars(&self) -> Vec<AnyView> {
        self.opened.iter().flat_map(|part| part.bars()).collect()
    }

    pub fn overlays(&self) -> Vec<AnyView> {
        self.opened
            .iter()
            .flat_map(|part| part.overlays())
            .collect()
    }

    pub fn list_view(&self, scene: &Scene, cx: &App) -> Option<AnyView> {
        self.opened
            .iter()
            .find_map(|part| part.list_view(scene, cx))
    }

    pub fn row_icon(&self, entry: &Entry, cx: &App) -> Option<AnyElement> {
        self.opened.iter().find_map(|part| part.row_icon(entry, cx))
    }

    pub fn open_entry(&self, entry: &Entry, path: &Path, cx: &mut App) -> bool {
        self.opened
            .iter()
            .any(|part| part.open_entry(entry, path, cx))
    }

    pub fn part<P: Part>(&self) -> Option<Entity<P>> {
        self.opened.iter().find_map(|part| {
            part.as_any()
                .downcast_ref::<Opened<P>>()
                .map(|opened| opened.entity.clone())
        })
    }

    pub fn observe(&self, changed: impl Fn(&mut App) + 'static, cx: &mut App) -> Vec<Subscription> {
        let changed: Rc<dyn Fn(&mut App)> = Rc::new(changed);
        self.opened
            .iter()
            .map(|part| part.observe(Rc::clone(&changed), cx))
            .collect()
    }
}
