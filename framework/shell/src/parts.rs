use gpui::{App, AppContext as _, Entity, Global};
use nova_files_messages::{Envelope, MessageType};

use crate::host::Host;
use crate::part::{Part, Slots};

trait OpenPart {
    fn deliver(&self, envelope: Envelope, cx: &mut App) -> Result<(), Envelope>;
}

struct Opened<P: Part> {
    entity: Entity<P>,
    slots: Slots<P>,
}

impl<P: Part> OpenPart for Opened<P> {
    fn deliver(&self, envelope: Envelope, cx: &mut App) -> Result<(), Envelope> {
        let Some(slot) = self.slots.events.get(&envelope.kind()) else {
            return Err(envelope);
        };
        self.entity.update(cx, |part, cx| slot(part, envelope, cx));
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct Kind {
    open: fn(&Host, &mut App) -> Box<dyn OpenPart>,
    takes: fn() -> Vec<MessageType>,
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
        });
    }

    pub fn takes(&self) -> Vec<MessageType> {
        self.kinds.iter().flat_map(|kind| (kind.takes)()).collect()
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
    Box::new(Opened {
        entity,
        slots: slots_of::<P>(),
    })
}

fn takes<P: Part>() -> Vec<MessageType> {
    slots_of::<P>().takes()
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
}
