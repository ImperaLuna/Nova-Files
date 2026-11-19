use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use crate::connection::{ClientId, CoreMessage, EventSink};
use crate::envelope::Envelope;
use crate::routes::{Routed, Routes};

pub trait Served: 'static {
    type Client: 'static;
    fn client(&self, events: Arc<dyn EventSink>) -> Self::Client;
    fn busy(&self) -> bool;
    fn offered(&self, offer: Envelope);
}

pub fn serve<S: Served>(
    receiver: &Receiver<CoreMessage>,
    idle_exit: Duration,
    routes: &Routes<S, S::Client>,
    state: &S,
) {
    let mut clients: HashMap<ClientId, S::Client> = HashMap::new();
    let mut stopping = false;
    loop {
        let idle = clients.is_empty() && !state.busy();
        if stopping && idle {
            return;
        }
        let message = if idle {
            match receiver.recv_timeout(idle_exit) {
                Ok(message) => message,
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match receiver.recv() {
                Ok(message) => message,
                Err(_) => return,
            }
        };
        if matches!(message, CoreMessage::Stop) {
            stopping = true;
        }
        handle(&mut clients, message, routes, state);
    }
}

fn handle<S: Served>(
    clients: &mut HashMap<ClientId, S::Client>,
    message: CoreMessage,
    routes: &Routes<S, S::Client>,
    state: &S,
) {
    match message {
        CoreMessage::Connected { client, events } => {
            clients.insert(client, state.client(Arc::from(events)));
        }
        CoreMessage::Disconnected { client } => {
            clients.remove(&client);
            routes.leave(state, client);
        }
        CoreMessage::Activity | CoreMessage::Stop => {}
        CoreMessage::Offer(offer) => state.offered(offer),
        CoreMessage::Command { client, command } => {
            let Some(target) = clients.get(&client) else {
                return;
            };
            match routes.route(state, client, target, command) {
                Ok(Routed::Handled) => {}
                Ok(Routed::Unrouted(kind)) => log::error!("no route for {}", kind.name()),
                Err(_) => {
                    clients.remove(&client);
                    routes.leave(state, client);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
