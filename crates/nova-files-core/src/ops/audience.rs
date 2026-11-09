//! The clients that watch jobs, for news about them.
//!
//! A job is no one client's: it keeps running when the window that started
//! it closes, and every window may follow it. The core's loop adds a client
//! here when it asks to watch jobs or starts one, and removes it when it
//! leaves; job threads read the list each time they report.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use nova_files_api::{ClientId, Event, EventSink};

/// The clients a job's news goes to. Cheap to clone; clones share the list.
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

    /// Tells every client `event`. A client that stopped listening is left
    /// for the loop to forget when its connection drops.
    pub fn tell(&self, event: &Event) {
        // Delivered outside the lock, so a slow client never holds up the
        // loop adding or removing another.
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
