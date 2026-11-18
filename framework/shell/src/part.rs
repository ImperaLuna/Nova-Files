use std::collections::HashMap;

use gpui::Context;
use nova_files_messages::{Envelope, Message, MessageType};

use crate::host::Host;

pub trait Part: Sized + 'static {
    fn open(host: &Host, cx: &mut Context<Self>) -> Self;
    fn slots(slots: &mut Slots<Self>);
}

type EventSlot<P> = Box<dyn Fn(&mut P, Envelope, &mut Context<P>)>;

pub struct Slots<P> {
    pub(crate) events: HashMap<MessageType, EventSlot<P>>,
}

impl<P> Default for Slots<P> {
    fn default() -> Self {
        Self {
            events: HashMap::new(),
        }
    }
}

impl<P: Part> Slots<P> {
    pub fn on_event<E: Message>(&mut self, take: fn(&mut P, E, &mut Context<P>)) {
        self.events.insert(
            MessageType::of::<E>(),
            Box::new(move |part, envelope, cx| {
                if let Some(event) = envelope.taken_as::<E>() {
                    take(part, event, cx);
                }
            }),
        );
    }

    pub fn takes(&self) -> Vec<MessageType> {
        self.events.keys().copied().collect()
    }
}
