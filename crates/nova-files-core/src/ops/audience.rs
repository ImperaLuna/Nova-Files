use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use nova_files_api::{ClientId, Event, EventSink};

#[derive(Clone, Default)]
pub struct Audience(Arc<Mutex<HashMap<ClientId, Arc<dyn EventSink>>>>);

impl Audience {
    pub fn join(&self, client: ClientId, events: Arc<dyn EventSink>) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(client, events);
    }

    pub fn leave(&self, client: ClientId) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&client);
    }

    pub fn tell(&self, event: &Event) {
        let clients: Vec<Arc<dyn EventSink>> = self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .cloned()
            .collect();
        for events in clients {
            events.deliver(event.clone()).ok();
        }
    }
}
