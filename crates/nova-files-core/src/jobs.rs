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
//! client shows. The listing itself reads names and kinds only, unless
//! the client asked for an order by modified time or size: then it reads
//! every entry's details before it answers (see `order`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;

use nova_files_api::{
    ClientGone, Entry, EntryId, Event, EventSink, HiddenFiles, Listing, ListingId, NotOpened,
    Order, ThumbnailSize, Unreadable,
};

use crate::details_queue;
use crate::file_icons::FileIcons;
use crate::thumbs::{Queue, Thumbnails};

use crate::list::{self, Stopped};
use crate::memory;
use crate::mime::MimeTypes;
use crate::open;
use crate::order;
use crate::places::Home;
use crate::preview;
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
    /// The preview that counts (see `preview`).
    current_preview: Arc<preview::Current>,
    /// `None` when its thread couldn't start: rows then show no details.
    details: Option<Arc<details_queue::Queue>>,
    /// `None` without a cache folder, or when its thread couldn't start.
    thumbnails: Option<Arc<Queue>>,
}

impl Client {
    pub fn new(
        events: Arc<dyn EventSink>,
        thumbnails: Option<&Thumbnails>,
        icons: &Arc<FileIcons>,
    ) -> Self {
        let thumbnails = thumbnails
            .and_then(|setup| Queue::start(setup.clone(), Arc::clone(&events)))
            .map(Arc::new);
        let details =
            details_queue::Queue::start(Arc::clone(&events), Arc::clone(icons)).map(Arc::new);
        Self {
            events,
            details,
            current_listing: Arc::new(AtomicU64::new(NO_LISTING)),
            current_search: Arc::new(AtomicU64::new(NO_LISTING)),
            current_preview: Arc::new(preview::Current::new()),
            thumbnails,
        }
    }

    /// The client shows `count` entries of listing `id` from `first`, or
    /// is about to: it wants their details, and their thumbnails at
    /// `thumbnails` while thumbnails are shown. With `None` (hidden) none
    /// are made, and any still queued are dropped.
    pub fn want_rows(
        &self,
        id: ListingId,
        first: EntryId,
        count: u32,
        thumbnails: Option<ThumbnailSize>,
    ) {
        if let Some(queue) = &self.details {
            queue.want(id, first, count);
        }
        if let Some(queue) = &self.thumbnails {
            // Hidden asks for none, which leaves the queue's size as it was.
            let (count, size) = thumbnails.map_or((0, ThumbnailSize::Normal), |size| (count, size));
            queue.want(id, first, count, size);
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

    /// The preview that counts.
    pub const fn current_preview(&self) -> &Arc<preview::Current> {
        &self.current_preview
    }

    /// Stops the client's listing, if one is running.
    fn cancel_listing(&self) {
        self.current_listing.store(NO_LISTING, Ordering::SeqCst);
    }
}

/// A client the core forgets (it left, or stopped listening) needs no more
/// answers, so its listing, search and preview stop too.
impl Drop for Client {
    fn drop(&mut self) {
        self.cancel_listing();
        self.current_search.store(NO_LISTING, Ordering::SeqCst);
        self.current_preview.cancel();
    }
}

/// A job that answers with listing `id`, the client's listing that
/// counts until another is asked for (see [`Client::start_listing`]).
pub struct ListingJob {
    id: ListingId,
    events: Arc<dyn EventSink>,
    current: Arc<AtomicU64>,
    details: Option<Arc<details_queue::Queue>>,
    thumbnails: Option<Arc<Queue>>,
}

impl ListingJob {
    /// Whether a newer listing replaced this one, or the client left.
    pub fn cancelled(&self) -> bool {
        self.current.load(Ordering::SeqCst) != self.id.0
    }

    /// Sends `event` unless the job was cancelled.
    pub fn answer(&self, event: Event) {
        if !self.cancelled() {
            self.events.deliver(event).ok();
        }
    }

    /// Sends `listing` unless the job was cancelled, first telling the
    /// details and thumbnail queues it is the one on screen, so the
    /// client's first rows request finds it.
    pub fn send(&self, listing: Listing) {
        if self.cancelled() {
            return;
        }
        let current = || !self.cancelled();
        if let Some(queue) = &self.details {
            queue.show(&listing, current);
        }
        if let Some(queue) = &self.thumbnails {
            queue.show(&listing, current);
        }
        self.events.deliver(Event::FolderListed(listing)).ok();
    }
}

impl Client {
    /// Makes listing `id` the one that counts, cancelling the one before.
    /// The details and thumbnail queues keep the listing on screen until
    /// this one is sent: a folder that can't be read leaves the client
    /// where it was. A listing sent but dropped by the client as stale
    /// has moved them on; the window then reads its folder again (see its
    /// module).
    pub fn start_listing(&self, id: ListingId) -> ListingJob {
        self.current_listing.store(id.0, Ordering::SeqCst);
        ListingJob {
            id,
            events: Arc::clone(&self.events),
            current: Arc::clone(&self.current_listing),
            details: self.details.clone(),
            thumbnails: self.thumbnails.clone(),
        }
    }
}

/// Lists `path` for `client` as listing `id` in `order`, with its hidden
/// entries as `hidden` says, replacing any listing before.
pub fn list_folder(
    client: &Client,
    id: ListingId,
    path: PathBuf,
    order: Order,
    hidden: HiddenFiles,
) {
    let job = client.start_listing(id);
    let job_path = path.clone();
    let spawned = thread::Builder::new()
        .name(LIST_THREAD.to_owned())
        .spawn(move || {
            let cancelled = || job.cancelled();
            let listed = list::read_folder(&job_path, cancelled).map(|entries| {
                let entries = list::keep_shown(entries, hidden);
                order::arrange(&job_path, entries, order, cancelled)
            });
            let entries: Arc<[Entry]> = match listed {
                Ok(Some(entries)) => entries.into(),
                Err(Stopped::Unreadable(reason)) => {
                    job.answer(Event::FolderUnreadable {
                        id,
                        path: job_path,
                        reason,
                    });
                    return;
                }
                // Nobody wants the answer any more.
                Ok(None) | Err(Stopped::Cancelled) => return,
            };
            job.send(Listing {
                id,
                path: job_path,
                entries: Arc::clone(&entries),
                folders: None,
            });
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
    let failed_path = path.clone();
    let spawned = thread::Builder::new()
        .name(OPEN_THREAD.to_owned())
        .spawn(move || {
            let types = types.get_or_init(MimeTypes::from_env);
            let event = match open::open_file(&path, types, activation.as_deref()) {
                Ok(()) => Event::FileOpened { path },
                Err(reason) => Event::FileNotOpened { path, reason },
            };
            events.deliver(event).ok();
        });
    if let Err(error) = spawned {
        // The portal can still open it.
        log::warn!("can't start a thread to open a file: {error}");
        client
            .deliver(Event::FileNotOpened {
                path: failed_path,
                reason: NotOpened::Failed,
            })
            .ok();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    #[test]
    fn a_replaced_listing_answers_nothing() {
        let (sender, events) = mpsc::channel();
        let client = Client::new(Arc::new(sender), None, &FileIcons::plain());
        let older = client.start_listing(ListingId(1));
        let newer = client.start_listing(ListingId(2));

        assert!(older.cancelled());
        assert!(!newer.cancelled());
        older.answer(Event::FolderListed(Listing {
            id: ListingId(1),
            path: PathBuf::from("/"),
            entries: Arc::new([]),
            folders: None,
        }));
        assert!(events.try_recv().is_err());
    }
}
