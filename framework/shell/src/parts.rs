use std::any::{Any, TypeId};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{
    AnyElement, AnyView, App, AppContext as _, Div, Entity, FocusHandle, Global, KeyBinding,
    Pixels, SharedString, Size, Subscription,
};
use nova_files_api::{Entry, Event, Listing, ListingId, Order};

use crate::assets::Icon;
use crate::jump::Jump;
use nova_files_messages::{Envelope, MessageType};

use crate::command::CommandSpec;
use crate::host::Host;
use crate::location::{Location, PageRef};
use crate::menu::{MenuAt, Placed, RowItem, SidebarAt, SidebarRow};
use crate::part::{Card, DropOver, Part, Slots};
use crate::scene::Scene;
use crate::slot_view::{Draw, SlotView};
use crate::text::TextField;

mod window;

pub use window::WindowParts;

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
    fn deliver(&self, envelope: &Envelope, cx: &mut App) -> bool;
    fn bind(&self, root: Div) -> Div;
    fn applies(&self, spec: &CommandSpec, scene: &Scene, cx: &App) -> Option<bool>;
    fn menu(&self, at: MenuAt, scene: &Scene, cx: &App) -> Vec<Placed>;
    fn page(&self, page: &PageRef) -> Option<AnyView>;
    fn column(&self, scene: &Scene, cx: &App) -> Option<ShownColumn>;
    fn sidebar(&self, at: SidebarAt, scene: &Scene, cx: &App) -> Vec<SidebarRow>;
    fn start_cards(&self) -> Vec<AnyView>;
    fn bars(&self) -> Vec<AnyView>;
    fn overlays(&self) -> Vec<AnyView>;
    fn list_view(&self, scene: &Scene, cx: &App) -> Option<AnyView>;
    fn row_icon(&self, entry: &Entry, side: Pixels, cx: &App) -> Option<AnyElement>;
    fn open_entry(&self, entry: &Entry, path: &Path, cx: &mut App) -> bool;
    fn typed(&self, jump: &Jump, scene: &Scene, cx: &mut App) -> bool;
    fn closing(&self, cx: &mut App);
    fn observe(&self, changed: Rc<dyn Fn(&mut App)>, cx: &mut App) -> Subscription;
    fn watch(&self, event: &Event, cx: &mut App);
    fn empty_note(&self, scene: &Scene, cx: &App) -> Option<(Icon, SharedString)>;
    fn name_cell(&self, entry: &Entry, cx: &App) -> Option<AnyElement>;
    fn name_field(&self, entry: &Entry, cx: &App) -> Option<Entity<TextField>>;
    fn busy(&self, cx: &App) -> bool;
    fn lists(&self, page: &PageRef) -> bool;
    fn list_page(&self, page: &PageRef, id: ListingId, order: Order) -> Option<Envelope>;
    fn stage(&self, scene: &Scene, room: Size<Pixels>, cx: &App) -> Option<AnyElement>;
    fn listed(&self, listing: &Listing, reordered: bool, cx: &mut App);
    fn takes_drops(&self, page: &PageRef) -> bool;
    fn drop_on(
        &self,
        page: &PageRef,
        paths: Vec<PathBuf>,
        cx: &mut App,
    ) -> Result<(), Vec<PathBuf>>;
    fn row_menu(&self, row: &str, cx: &App) -> Vec<Vec<RowItem>>;
    fn section_buttons(
        &self,
        at: SidebarAt,
        scene: &Scene,
        cx: &App,
    ) -> Vec<(&'static str, RowItem)>;
    fn drop_label(&self, over: &DropOver<'_>, cx: &App) -> Option<SharedString>;
    fn drop_at(&self, over: &DropOver<'_>, cx: &mut App) -> bool;
    fn as_any(&self) -> &dyn Any;
    fn selectable(&self, entry: &Entry, cx: &App) -> bool;
    fn several(&self, cx: &App) -> bool;
    fn dims(&self, entry: &Entry, cx: &App) -> bool;
    fn focus(&self, cx: &App) -> Option<FocusHandle>;
    fn refused(&self, id: ListingId, path: &Path, cx: &mut App);
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
    fn deliver(&self, envelope: &Envelope, cx: &mut App) -> bool {
        let Some(slot) = self.slots.events.get(&envelope.kind()) else {
            return false;
        };
        self.entity.update(cx, |part, cx| slot(part, envelope, cx));
        true
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

    fn menu(&self, at: MenuAt, scene: &Scene, cx: &App) -> Vec<Placed> {
        let part = self.entity.read(cx);
        self.slots
            .menus
            .iter()
            .filter(|(place, ..)| *place == at)
            .map(|&(_, group, rank, items)| Placed::new(group, rank, items(part, scene, cx)))
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

    fn row_icon(&self, entry: &Entry, side: Pixels, cx: &App) -> Option<AnyElement> {
        let part = self.entity.read(cx);
        self.slots
            .row_icons
            .iter()
            .find_map(|icon| icon(part, entry, side, cx))
    }

    fn closing(&self, cx: &mut App) {
        if self.slots.closing.is_empty() {
            return;
        }
        self.entity.update(cx, |part, cx| {
            for closing in &self.slots.closing {
                closing(part, cx);
            }
        });
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

    fn typed(&self, jump: &Jump, scene: &Scene, cx: &mut App) -> bool {
        if self.slots.typed.is_empty() {
            return false;
        }
        self.entity.update(cx, |part, cx| {
            self.slots
                .typed
                .iter()
                .any(|typed| typed(part, jump, scene, cx))
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

    fn name_field(&self, entry: &Entry, cx: &App) -> Option<Entity<TextField>> {
        let part = self.entity.read(cx);
        self.slots
            .name_fields
            .iter()
            .find_map(|field| field(part, entry, cx))
    }

    fn busy(&self, cx: &App) -> bool {
        let part = self.entity.read(cx);
        self.slots.busy.iter().any(|busy| busy(part, cx))
    }

    fn lists(&self, page: &PageRef) -> bool {
        self.slots
            .listing_pages
            .iter()
            .any(|(kind, _)| *kind == page.kind())
    }

    fn list_page(&self, page: &PageRef, id: ListingId, order: Order) -> Option<Envelope> {
        self.slots
            .listing_pages
            .iter()
            .find_map(|(_, list)| list(page, id, order))
    }

    fn stage(&self, scene: &Scene, room: Size<Pixels>, cx: &App) -> Option<AnyElement> {
        let part = self.entity.read(cx);
        self.slots
            .stages
            .iter()
            .find_map(|stage| stage(part, scene, room, cx))
    }

    fn listed(&self, listing: &Listing, reordered: bool, cx: &mut App) {
        if self.slots.listed.is_empty() {
            return;
        }
        self.entity.update(cx, |part, cx| {
            for listed in &self.slots.listed {
                listed(part, listing, reordered, cx);
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

    fn row_menu(&self, row: &str, cx: &App) -> Vec<Vec<RowItem>> {
        let part = self.entity.read(cx);
        self.slots
            .row_menus
            .iter()
            .flat_map(|items| items(part, row, cx))
            .collect()
    }

    fn section_buttons(
        &self,
        at: SidebarAt,
        scene: &Scene,
        cx: &App,
    ) -> Vec<(&'static str, RowItem)> {
        let part = self.entity.read(cx);
        self.slots
            .head_buttons
            .iter()
            .filter(|(place, _, _)| *place == at)
            .filter_map(|(_, id, button)| button(part, scene, cx).map(|item| (*id, item)))
            .collect()
    }

    fn drop_label(&self, over: &DropOver<'_>, cx: &App) -> Option<SharedString> {
        let part = self.entity.read(cx);
        self.slots
            .drop_targets
            .iter()
            .find_map(|(aim, _)| aim(part, over, cx))
    }

    fn drop_at(&self, over: &DropOver<'_>, cx: &mut App) -> bool {
        let part = self.entity.read(cx);
        let Some(&(_, take)) = self
            .slots
            .drop_targets
            .iter()
            .find(|(aim, _)| aim(part, over, cx).is_some())
        else {
            return false;
        };
        let (at, sources, modifiers) = (over.at.clone(), over.sources.to_vec(), over.modifiers);
        self.entity
            .update(cx, |part, cx| take(part, at, sources, modifiers, cx));
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn selectable(&self, entry: &Entry, cx: &App) -> bool {
        let part = self.entity.read(cx);
        self.slots
            .selectable
            .iter()
            .all(|selectable| selectable(part, entry, cx))
    }

    fn several(&self, cx: &App) -> bool {
        let part = self.entity.read(cx);
        self.slots.several.iter().all(|several| several(part, cx))
    }

    fn dims(&self, entry: &Entry, cx: &App) -> bool {
        let part = self.entity.read(cx);
        self.slots.dims.iter().any(|dims| dims(part, entry, cx))
    }

    fn focus(&self, cx: &App) -> Option<FocusHandle> {
        let part = self.entity.read(cx);
        self.slots.focus.iter().find_map(|focus| focus(part, cx))
    }

    fn refused(&self, id: ListingId, path: &Path, cx: &mut App) {
        if self.slots.refused.is_empty() {
            return;
        }
        self.entity.update(cx, |part, cx| {
            for refused in &self.slots.refused {
                refused(part, id, path, cx);
            }
        });
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

#[derive(Default)]
struct BoundKeys(HashSet<&'static str>);

impl Global for BoundKeys {}

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
        each_once(self.kinds.iter().flat_map(|kind| (kind.takes)()))
    }

    pub fn sends(&self) -> Vec<MessageType> {
        each_once(self.kinds.iter().flat_map(|kind| (kind.sends)()))
    }

    fn bind_new_keys(&self, cx: &mut App) {
        let bound = cx.default_global::<BoundKeys>();
        let new: Vec<_> = self
            .kinds
            .iter()
            .flat_map(|kind| (kind.specs)())
            .filter(|spec| bound.0.insert(spec.id))
            .collect();
        cx.bind_keys(new.into_iter().flat_map(CommandSpec::bindings));
    }

    pub fn bindings(&self) -> Vec<KeyBinding> {
        self.kinds
            .iter()
            .flat_map(|kind| (kind.specs)())
            .flat_map(CommandSpec::bindings)
            .collect()
    }

    pub fn open(&self, host: &Host, cx: &mut App) -> WindowParts {
        self.bind_new_keys(cx);
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

fn each_once(types: impl Iterator<Item = MessageType>) -> Vec<MessageType> {
    let mut once = Vec::new();
    for kind in types {
        if !once.contains(&kind) {
            once.push(kind);
        }
    }
    once
}
