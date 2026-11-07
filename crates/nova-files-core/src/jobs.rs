//! Work that touches the disk, each on a thread of its own, answering the
//! client directly when done.
//!
//! A client has at most one folder listing that counts: asking for another
//! (or leaving) cancels the one before. A listing job checks before each
//! read and before it answers, so a cancelled job stops early and its late
//! result is never sent. The client also drops answers whose id isn't its
//! newest, which covers the moment between that check and the send.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

use nova_files_api::{ClientGone, Event, EventSink, Listing, ListingId, Unreadable};

use crate::list::{self, Stopped};
use crate::places;

const LIST_THREAD: &str = "nova-files-list";
const PLACES_THREAD: &str = "nova-files-places";

/// No listing counts: the client never asked, cancelled, or left.
const NO_LISTING: u64 = u64::MAX;

/// What the core keeps for one connected client.
pub struct Client {
    events: Arc<dyn EventSink>,
    /// The id of the listing that counts, or [`NO_LISTING`].
    current_listing: Arc<AtomicU64>,
}

impl Client {
    pub fn new(events: Arc<dyn EventSink>) -> Self {
        Self {
            events,
            current_listing: Arc::new(AtomicU64::new(NO_LISTING)),
        }
    }

    pub fn deliver(&self, event: Event) -> Result<(), ClientGone> {
        self.events.deliver(event)
    }

    /// Stops the client's listing, if one is running.
    pub fn cancel_listing(&self) {
        self.current_listing.store(NO_LISTING, Ordering::SeqCst);
    }
}

/// Lists `path` for `client` as listing `id`, replacing any listing before.
pub fn list_folder(client: &Client, id: ListingId, path: PathBuf) {
    client.current_listing.store(id.0, Ordering::SeqCst);
    let events = Arc::clone(&client.events);
    let current = Arc::clone(&client.current_listing);
    let job_path = path.clone();
    let spawned = thread::Builder::new()
        .name(LIST_THREAD.to_owned())
        .spawn(move || {
            let cancelled = || current.load(Ordering::SeqCst) != id.0;
            let event = match list::read_folder(&job_path, cancelled) {
                Ok(entries) => Event::FolderListed(Listing {
                    id,
                    path: job_path,
                    entries: entries.into(),
                }),
                Err(Stopped::Unreadable(reason)) => Event::FolderUnreadable {
                    id,
                    path: job_path,
                    reason,
                },
                Err(Stopped::Cancelled) => return,
            };
            if !cancelled() {
                // A client that left needs no answer.
                events.deliver(event).ok();
            }
        });
    if spawned.is_err() {
        client
            .deliver(Event::FolderUnreadable {
                id,
                path,
                reason: Unreadable::Failed,
            })
            .ok();
    }
}

/// Finds the places for `client`.
pub fn places(client: &Client) {
    let events = Arc::clone(&client.events);
    let spawned = thread::Builder::new()
        .name(PLACES_THREAD.to_owned())
        .spawn(move || {
            events.deliver(Event::Places(places::places())).ok();
        });
    if spawned.is_err() {
        // Without a thread there are no places to offer; the list is empty
        // rather than missing.
        client.deliver(Event::Places(Vec::new())).ok();
    }
}
