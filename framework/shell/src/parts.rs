use std::any::TypeId;
use std::rc::Rc;

use gpui::{
    AnyView, App, AppContext as _, Div, Entity, Global, KeyBinding, SharedString, Subscription,
};
use nova_files_api::Entry;
use nova_files_messages::{Envelope, MessageType};

use crate::command::CommandSpec;
use crate::host::Host;
use crate::location::{Location, PageRef};
use crate::menu::{MenuAt, MenuItem, SidebarAt, SidebarRow};
use crate::part::{Part, Slots};
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
    fn observe(&self, changed: Rc<dyn Fn(&mut App)>, cx: &mut App) -> Subscription;
}

struct Opened<P: Part> {
    entity: Entity<P>,
    host: Host,
    slots: Slots<P>,
    pages: Vec<(TypeId, AnyView)>,
    cards: Vec<AnyView>,
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

    fn observe(&self, changed: Rc<dyn Fn(&mut App)>, cx: &mut App) -> Subscription {
        cx.observe(&self.entity, move |_, cx| changed(cx))
    }
}

#[derive(Clone, Copy)]
struct Kind {
    open: fn(&Host, &mut App) -> Box<dyn OpenPart>,
    takes: fn() -> Vec<MessageType>,
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
            specs: specs::<P>,
        });
    }

    pub fn takes(&self) -> Vec<MessageType> {
        self.kinds.iter().flat_map(|kind| (kind.takes)()).collect()
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
    let cards = slots
        .start_cards
        .iter()
        .map(|&card| {
            let (part, host) = (entity.clone(), host.clone());
            let draw: Draw = Box::new(move |window, cx| {
                let scene = host.scene(cx);
                part.update(cx, |part, cx| card(part, &scene, window, cx))
            });
            slot_view(&entity, draw, false, cx)
        })
        .collect();
    Box::new(Opened {
        entity,
        host: host.clone(),
        slots,
        pages,
        cards,
    })
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

    pub fn observe(&self, changed: impl Fn(&mut App) + 'static, cx: &mut App) -> Vec<Subscription> {
        let changed: Rc<dyn Fn(&mut App)> = Rc::new(changed);
        self.opened
            .iter()
            .map(|part| part.observe(Rc::clone(&changed), cx))
            .collect()
    }
}
