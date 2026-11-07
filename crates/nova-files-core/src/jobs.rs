//! Work that touches the disk, each on a thread of its own, answering the
//! client directly when done.
//!
//! A client has at most one folder listing that counts: asking for another
//! (or leaving) cancels the one before. A listing job checks before each
//! read and before it answers, so a cancelled job stops early and its late
//! result is never sent. The client also drops answers whose id isn't its
//! newest, which covers the moment between that check and the send.
//!
//! After the listing, the same job reads the entries' details and sends
//! them in batches, under the same cancel rule.
//!
//! Each client also has a thumbnail queue (see `thumbs`), told which
//! listing is on screen as soon as its listing has been sent.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

use nova_files_api::{
    ClientGone, Entry, EntryId, Event, EventSink, Listing, ListingId, Unreadable,
};

use crate::details;
use crate::thumbs::{Queue, Thumbnails};

use crate::list::{self, Stopped};
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
    /// `None` without a cache folder, or when its thread couldn't start.
    thumbnails: Option<Arc<Queue>>,
}

impl Client {
    pub fn new(events: Arc<dyn EventSink>, thumbnails: Option<&Thumbnails>) -> Self {
        let thumbnails = thumbnails
            .and_then(|setup| Queue::start(setup.clone(), Arc::clone(&events)))
            .map(Arc::new);
        Self {
            events,
            current_listing: Arc::new(AtomicU64::new(NO_LISTING)),
            thumbnails,
        }
    }

    /// The client wants thumbnails for `count` entries of listing `id`.
    pub fn want_thumbnails(&self, id: ListingId, first: EntryId, count: u32) {
        if let Some(queue) = &self.thumbnails {
            queue.want(id, first, count);
        }
    }

    pub fn deliver(&self, event: Event) -> Result<(), ClientGone> {
        self.events.deliver(event)
    }

    /// Stops the client's listing, if one is running.
    fn cancel_listing(&self) {
        self.current_listing.store(NO_LISTING, Ordering::SeqCst);
    }
}

/// A client the core forgets (it left, or stopped listening) needs no more
/// answers, so its listing stops too.
impl Drop for Client {
    fn drop(&mut self) {
        self.cancel_listing();
    }
}

/// Lists `path` for `client` as listing `id`, replacing any listing before.
pub fn list_folder(client: &Client, id: ListingId, path: PathBuf) {
    client.current_listing.store(id.0, Ordering::SeqCst);
    if let Some(queue) = &client.thumbnails {
        queue.clear();
    }
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
            // A client that left needs no answer, and no details.
            if events.deliver(listing).is_ok() {
                if let Some(queue) = &thumbnails
                    && !cancelled()
                {
                    queue.show(id, job_path.clone(), Arc::clone(&entries));
                }
                send_details(events.as_ref(), id, &job_path, &entries, cancelled);
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

/// Reads the details of `entries` in the folder at `folder` and sends them
/// in batches, until done, cancelled or the client is gone.
fn send_details(
    events: &dyn EventSink,
    id: ListingId,
    folder: &Path,
    entries: &[Entry],
    cancelled: impl Fn() -> bool,
) {
    for (index, batch) in entries.chunks(details::BATCH).enumerate() {
        let Some(read) = details::read_batch(folder, batch, &cancelled) else {
            return;
        };
        let first = index * details::BATCH;
        let Ok(first) = u32::try_from(first) else {
            return;
        };
        if cancelled() {
            return;
        }
        let event = Event::DetailsRead {
            id,
            first: EntryId(first),
            details: read.into(),
        };
        if events.deliver(event).is_err() {
            return;
        }
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
