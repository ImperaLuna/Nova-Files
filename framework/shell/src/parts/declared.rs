use nova_files_messages::MessageType;

use crate::command::CommandSpec;
use crate::part::{Part, Slots};

pub(super) fn slots_of<P: Part>() -> Slots<P> {
    let mut slots = Slots::default();
    P::slots(&mut slots);
    slots
}

pub(super) fn takes<P: Part>() -> Vec<MessageType> {
    slots_of::<P>().takes()
}

pub(super) fn sends<P: Part>() -> Vec<MessageType> {
    slots_of::<P>().sent()
}

pub(super) fn specs<P: Part>() -> Vec<&'static CommandSpec> {
    slots_of::<P>().specs()
}

pub(super) fn each_once(types: impl Iterator<Item = MessageType>) -> Vec<MessageType> {
    let mut once = Vec::new();
    for kind in types {
        if !once.contains(&kind) {
            once.push(kind);
        }
    }
    once
}
