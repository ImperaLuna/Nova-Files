//! Work that touches the disk, each on a thread of its own, answering the
//! client directly when done.
//!
//! A client has at most one folder listing that counts: asking for another
//! (or leaving) cancels the one before. A listing job checks before each
//! read and before it answers, so a cancelled job stops early and its late
//! result is never sent. The client also drops answers whose id isn't its
//! newest, which covers the moment between that check and the send.
//!
//! Each client also has a details queue (see `details_queue`) and a
//! thumbnail queue (see `thumbs`), both told which listing is on screen
//! just before its listing is sent, and both told which of its rows the
//! client shows. The listing itself reads names and kinds only.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

use nova_files_api::{
    ClientGone, Entry, EntryId, Event, EventSink, Listing, ListingId, Unreadable,
};

use crate::details_queue;
use crate::thumbs::{Queue, Thumbnails};

use crate::list::{self, Stopped};
use crate::memory;
use crate::places::{self, Home};

const LIST_THREAD: &str = "nova-files-list";
const PLACES_THREAD: &str = "nova-files-places";

/// No listing counts: the client never asked, cancelled, or left.
const NO_LISTING: u64 = u64::MAX;

/// What the core keeps for one connected client.
pub struct Client {
    events: Arc<dyn EventSink>,
    /// The id of the listing that counts, or [`NO_LISTING`].
    current_listing: Arc<AtomicU64>,
    /// The id of the search that counts, or [`NO_LISTING`].
    current_search: Arc<AtomicU64>,
    /// `None` when its thread couldn't start: rows then show no details.
    details: Option<Arc<details_queue::Queue>>,
    /// `None` without a cache folder, or when its thread couldn't start.
    thumbnails: Option<Arc<Queue>>,
}

impl Client {
    pub fn new(events: Arc<dyn EventSink>, thumbnails: Option<&Thumbnails>) -> Self {
        let thumbnails = thumbnails
            .and_then(|setup| Queue::start(setup.clone(), Arc::clone(&events)))
            .map(Arc::new);
        let details = details_queue::Queue::start(Arc::clone(&events)).map(Arc::new);
        Self {
            events,
            details,
            current_listing: Arc::new(AtomicU64::new(NO_LISTING)),
            current_search: Arc::new(AtomicU64::new(NO_LISTING)),
            thumbnails,
        }
    }

    /// The client shows `count` entries of listing `id` from `first`, or
    /// is about to: it wants their details and thumbnails.
    pub fn want_rows(&self, id: ListingId, first: EntryId, count: u32) {
        if let Some(queue) = &self.details {
            queue.want(id, first, count);
        }
        if let Some(queue) = &self.thumbnails {
            queue.want(id, first, count);
        }
    }

    pub fn deliver(&self, event: Event) -> Result<(), ClientGone> {
        self.events.deliver(event)
    }

    pub const fn events(&self) -> &Arc<dyn EventSink> {
        &self.events
    }

    /// The id of the client's search that counts.
    pub const fn current_search(&self) -> &Arc<AtomicU64> {
        &self.current_search
    }

    /// Stops the client's listing, if one is running.
    fn cancel_listing(&self) {
        self.current_listing.store(NO_LISTING, Ordering::SeqCst);
    }
}

/// A client the core forgets (it left, or stopped listening) needs no more
/// answers, so its listing and search stop too.
impl Drop for Client {
    fn drop(&mut self) {
        self.cancel_listing();
        self.current_search.store(NO_LISTING, Ordering::SeqCst);
    }
}

/// Lists `path` for `client` as listing `id`, replacing any listing before.
pub fn list_folder(client: &Client, id: ListingId, path: PathBuf) {
    // The details and thumbnail queues keep the listing on screen until
    // this one is sent: a folder that can't be read leaves the client where
    // it was. A listing sent but dropped by the client as stale has moved
    // them on; the window then reads its folder again (see its module).
    client.current_listing.store(id.0, Ordering::SeqCst);
    let details = client.details.clone();
    let thumbnails = client.thumbnails.clone();
    let events = Arc::clone(&client.events);
    let current = Arc::clone(&client.current_listing);
    let job_path = path.clone();
    let spawned = thread::Builder::new()
        .name(LIST_THREAD.to_owned())
        .spawn(move || {
            let cancelled = || current.load(Ordering::SeqCst) != id.0;
            let entries: Arc<[Entry]> = match list::read_folder(&job_path, cancelled) {
                Ok(entries) => entries.into(),
                Err(Stopped::Unreadable(reason)) => {
                    if !cancelled() {
                        events
                            .deliver(Event::FolderUnreadable {
                                id,
                                path: job_path,
                                reason,
                            })
                            .ok();
                    }
                    return;
                }
                Err(Stopped::Cancelled) => return,
            };
            if cancelled() {
                return;
            }
            let listing = Event::FolderListed(Listing {
                id,
                path: job_path.clone(),
                entries: Arc::clone(&entries),
            });
            if let Some(queue) = &details {
                queue.show(id, job_path.clone(), Arc::clone(&entries), || !cancelled());
            }
            if let Some(queue) = &thumbnails {
                queue.show(id, job_path.clone(), Arc::clone(&entries), || !cancelled());
            }
            events.deliver(listing).ok();
            // Reading the folder is the heaviest work the core does; most
            // of what it built is free again now.
            drop(entries);
            memory::give_back();
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

/// Finds the places under `home` for `client`.
pub fn places(client: &Client, home: Option<Home>) {
    let events = Arc::clone(&client.events);
    let spawned = thread::Builder::new()
        .name(PLACES_THREAD.to_owned())
        .spawn(move || {
            events
                .deliver(Event::Places(places::places(home.as_ref())))
                .ok();
        });
    if spawned.is_err() {
        // Without a thread there are no places to offer; the list is empty
        // rather than missing.
        client.deliver(Event::Places(Vec::new())).ok();
    }
}
