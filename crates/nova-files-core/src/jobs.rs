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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;

use nova_files_api::{
    ClientGone, Entry, EntryId, Event, EventSink, Listing, ListingId, Unreadable,
};

use nova_files_api::Thumbnails as ThumbnailsSetting;

use crate::details_queue;
use crate::thumbs::{Queue, Thumbnails};

use crate::list::{self, Stopped};
use crate::memory;
use crate::mime::MimeTypes;
use crate::open;
use crate::places::Home;
use crate::trash;

const LIST_THREAD: &str = "nova-files-list";
const OPEN_THREAD: &str = "nova-files-open";
const TRASH_THREAD: &str = "nova-files-trash";

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
    /// is about to: it wants their details, and their thumbnails while
    /// they are `Shown`. Hidden, none are made, and any still queued are
    /// dropped.
    pub fn want_rows(
        &self,
        id: ListingId,
        first: EntryId,
        count: u32,
        thumbnails: ThumbnailsSetting,
    ) {
        if let Some(queue) = &self.details {
            queue.want(id, first, count);
        }
        if let Some(queue) = &self.thumbnails {
            let count = match thumbnails {
                ThumbnailsSetting::Shown => count,
                ThumbnailsSetting::Hidden => 0,
            };
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

/// Answers with what is in each trash `home`'s user has.
///
/// # Errors
/// [`ClientGone`] when the client stopped listening.
pub fn list_trash(client: &Client, home: Option<Home>) -> Result<(), ClientGone> {
    let events = Arc::clone(&client.events);
    let spawned = thread::Builder::new()
        .name(TRASH_THREAD.to_owned())
        .spawn(move || {
            let listed = trash::Trashes::of(home.as_ref())
                .all()
                .iter()
                .flat_map(trash::list)
                .collect();
            events.deliver(Event::Trash(listed)).ok();
        });
    if spawned.is_err() {
        // As for places: an empty trash rather than no answer.
        return client.deliver(Event::Trash(Vec::new()));
    }
    Ok(())
}

/// Opens `path` in its default app, reading the MIME database into `types`
/// first if no open has yet, and answers whether the app started.
pub fn open_file(
    client: &Client,
    types: Arc<OnceLock<MimeTypes>>,
    path: PathBuf,
    activation: Option<String>,
) {
    let events = Arc::clone(&client.events);
    let spawned = thread::Builder::new()
        .name(OPEN_THREAD.to_owned())
        .spawn(move || {
            let types = types.get_or_init(MimeTypes::from_env);
            let event = match open::open_file(&path, types, activation.as_deref()) {
                Ok(()) => Event::FileOpened,
                Err(_) => Event::FileNotOpened,
            };
            events.deliver(event).ok();
        });
    if spawned.is_err() {
        // The portal can still open it.
        client.deliver(Event::FileNotOpened).ok();
    }
}
