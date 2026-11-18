use std::collections::HashMap;
use std::collections::hash_map::Entry;

use crate::connection::{ClientGone, ClientId};
use crate::envelope::{Envelope, Message, MessageType};

type Handler<S, C> = Box<dyn Fn(&S, ClientId, &C, Envelope) -> Result<(), ClientGone> + Send>;
type Leaver<S> = Box<dyn Fn(&S, ClientId) + Send>;

pub struct Routes<S, C> {
    by_type: HashMap<MessageType, Handler<S, C>>,
    leaving: Vec<Leaver<S>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Routed {
    Handled,
    Unrouted(MessageType),
}

impl<S: 'static, C: 'static> Routes<S, C> {
    pub fn new() -> Self {
        Self {
            by_type: HashMap::new(),
            leaving: Vec::new(),
        }
    }

    pub fn on<M: Message>(
        &mut self,
        handle: impl Fn(&S, ClientId, &C, M) -> Result<(), ClientGone> + Send + 'static,
    ) -> Result<(), MessageType> {
        match self.by_type.entry(MessageType::of::<M>()) {
            Entry::Occupied(taken) => Err(*taken.key()),
            Entry::Vacant(free) => {
                free.insert(Box::new(move |state, client, target, envelope| {
                    envelope
                        .open::<M>()
                        .map_or(Ok(()), |message| handle(state, client, target, message))
                }));
                Ok(())
            }
        }
    }

    pub fn on_leave(&mut self, forget: impl Fn(&S, ClientId) + Send + 'static) {
        self.leaving.push(Box::new(forget));
    }

    pub fn route(
        &self,
        state: &S,
        client: ClientId,
        target: &C,
        envelope: Envelope,
    ) -> Result<Routed, ClientGone> {
        let kind = envelope.kind();
        let Some(handle) = self.by_type.get(&kind) else {
            return Ok(Routed::Unrouted(kind));
        };
        handle(state, client, target, envelope).map(|()| Routed::Handled)
    }

    pub fn leave(&self, state: &S, client: ClientId) {
        for forget in &self.leaving {
            forget(state, client);
        }
    }

    pub fn takes(&self) -> impl Iterator<Item = MessageType> + '_ {
        self.by_type.keys().copied()
    }
}

impl<S: 'static, C: 'static> Default for Routes<S, C> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn missing_routes(sent: &[MessageType], taken: &[MessageType]) -> Vec<MessageType> {
    let mut missing: Vec<MessageType> = sent
        .iter()
        .filter(|kind| !taken.contains(kind))
        .copied()
        .collect();
    missing.dedup();
    missing
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[derive(Debug)]
    struct Add(u32);

    #[derive(Debug)]
    struct Clear;

    #[derive(Debug)]
    struct Unknown;

    #[derive(Default)]
    struct Total {
        sum: RefCell<u32>,
        left: RefCell<Vec<ClientId>>,
    }

    fn routes() -> Routes<Total, u32> {
        let mut routes = Routes::new();
        routes
            .on(|total: &Total, _, scale: &u32, Add(n)| {
                *total.sum.borrow_mut() += n * scale;
                Ok(())
            })
            .unwrap();
        routes
            .on(|_: &Total, _, _: &u32, Clear| Err(ClientGone))
            .unwrap();
        routes.on_leave(|total: &Total, client| total.left.borrow_mut().push(client));
        routes
    }

    #[test]
    fn each_message_goes_to_the_route_of_its_type() {
        let table = routes();
        let total = Total::default();
        let routed = table.route(&total, ClientId(1), &10, Envelope::new(Add(2)));
        assert_eq!(routed, Ok(Routed::Handled));
        assert_eq!(*total.sum.borrow(), 20);
        let gone = table.route(&total, ClientId(1), &10, Envelope::new(Clear));
        assert_eq!(gone, Err(ClientGone));
    }

    #[test]
    fn a_message_with_no_route_says_its_type() {
        let table = routes();
        let routed = table.route(&Total::default(), ClientId(1), &1, Envelope::new(Unknown));
        assert_eq!(routed, Ok(Routed::Unrouted(MessageType::of::<Unknown>())));
    }

    #[test]
    fn a_type_takes_one_route_only() {
        let mut routes = routes();
        let again = routes.on(|_: &Total, _, _: &u32, Add(_)| Ok(()));
        assert_eq!(again, Err(MessageType::of::<Add>()));
    }

    #[test]
    fn leaving_reaches_every_listener() {
        let routes = routes();
        let total = Total::default();
        routes.leave(&total, ClientId(4));
        assert_eq!(*total.left.borrow(), [ClientId(4)]);
    }

    #[test]
    fn missing_routes_names_each_type_sent_but_never_taken() {
        let taken: Vec<MessageType> = routes().takes().collect();
        let sent = [MessageType::of::<Add>(), MessageType::of::<Unknown>()];
        assert_eq!(
            missing_routes(&sent, &taken),
            [MessageType::of::<Unknown>()]
        );
    }
}
