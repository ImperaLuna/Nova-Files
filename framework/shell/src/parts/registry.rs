use std::collections::HashSet;
use std::rc::Rc;

use gpui::{AnyView, App, AppContext as _, Entity, Global, KeyBinding};
use nova_files_messages::MessageType;

use super::{OpenPart, Opened, WindowParts};
use crate::command::CommandSpec;
use crate::host::Host;
use crate::location::Location;
use crate::part::{Card, Part, Slots};
use crate::slot_view::{Draw, Fit, SlotView};

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
            (*kind, slot_view(&entity, draw, Fit::Fill, cx))
        })
        .collect();
    let ranks = slots.start_cards.iter().map(|(rank, _)| *rank);
    let drawn: Vec<Card<P>> = slots.start_cards.iter().map(|(_, card)| *card).collect();
    let cards = ranks
        .zip(card_views(&entity, host, &drawn, Fit::Content, cx))
        .collect();
    let bars = card_views(&entity, host, &slots.bars, Fit::Content, cx);
    let footers = card_views(&entity, host, &slots.footers, Fit::Content, cx);
    let overlays = card_views(&entity, host, &slots.overlays, Fit::Content, cx);
    let layers = card_views(&entity, host, &slots.layers, Fit::Cover, cx);
    let chip_ranks = slots.path_chips.iter().map(|(rank, _)| *rank);
    let chips: Vec<Card<P>> = slots.path_chips.iter().map(|(_, chip)| *chip).collect();
    let path_chips = chip_ranks
        .zip(card_views(&entity, host, &chips, Fit::Content, cx))
        .collect();
    let views: Vec<Card<P>> = slots.list_views.iter().map(|(_, draw)| *draw).collect();
    let list_views = card_views(&entity, host, &views, Fit::Fill, cx);
    Box::new(Opened {
        entity,
        host: host.clone(),
        slots,
        pages,
        cards,
        bars,
        footers,
        overlays,
        layers,
        path_chips,
        list_views,
    })
}

fn card_views<P: Part>(
    entity: &Entity<P>,
    host: &Host,
    cards: &[Card<P>],
    fit: Fit,
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
            slot_view(entity, draw, fit, cx)
        })
        .collect()
}

fn slot_view<P: Part>(part: &Entity<P>, draw: Draw, fit: Fit, cx: &mut App) -> AnyView {
    let part = part.clone();
    cx.new(|cx| {
        cx.observe(&part, |_, _, cx| cx.notify()).detach();
        SlotView::new(draw, fit)
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
