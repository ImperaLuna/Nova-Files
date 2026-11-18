use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;

use nova_files_api::{
    ClientGone, Details, Entry, EntryId, Event, EventSink, HiddenFiles, Listing, ListingId, Order,
    ThumbnailSize, Unreadable,
};

use nova_files_open_api::{self as open_api, NotOpened};

use crate::archive::ArchiveFailure;
use crate::details_queue;
use crate::file_icons::FileIcons;
use crate::inside::{self, Inside, Within};
use crate::thumbs::{Queue, Thumbnails};

use crate::list::{self, Stopped};
use crate::memory;
use crate::mime::MimeTypes;
use crate::open;
use crate::order;
use crate::preview;

const LIST_THREAD: &str = "nova-files-list";
const OPEN_THREAD: &str = "nova-files-open";

const NO_LISTING: u64 = u64::MAX;

pub struct Client {
    events: Arc<dyn EventSink>,
    current_listing: Arc<AtomicU64>,
    current_search: Arc<AtomicU64>,
    current_preview: Arc<preview::Current>,
    details: Option<Arc<details_queue::Queue>>,
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
            queue.want(id, first, thumbnails.map_or(0, |_| count), thumbnails);
        }
    }

    pub fn deliver(&self, event: Event) -> Result<(), ClientGone> {
        self.events.deliver(event)
    }

    pub const fn events(&self) -> &Arc<dyn EventSink> {
        &self.events
    }

    pub const fn current_search(&self) -> &Arc<AtomicU64> {
        &self.current_search
    }

    pub const fn current_preview(&self) -> &Arc<preview::Current> {
        &self.current_preview
    }

    fn cancel_listing(&self) {
        self.current_listing.store(NO_LISTING, Ordering::SeqCst);
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.cancel_listing();
        self.current_search.store(NO_LISTING, Ordering::SeqCst);
        self.current_preview.cancel();
    }
}

pub struct ListingJob {
    id: ListingId,
    events: Arc<dyn EventSink>,
    current: Arc<AtomicU64>,
    details: Option<Arc<details_queue::Queue>>,
    thumbnails: Option<Arc<Queue>>,
}

impl ListingJob {
    pub fn cancelled(&self) -> bool {
        self.current.load(Ordering::SeqCst) != self.id.0
    }

    pub fn answer(&self, event: Event) {
        if !self.cancelled() {
            self.events.deliver(event).ok();
        }
    }

    pub fn send(&self, listing: Listing) {
        self.send_known(listing, None);
    }

    pub fn send_known(&self, listing: Listing, known: Option<Arc<[Details]>>) {
        if self.cancelled() {
            return;
        }
        let current = || !self.cancelled();
        if let Some(queue) = &self.details {
            queue.show(&listing, known, current);
        }
        if let Some(queue) = &self.thumbnails {
            queue.show(&listing, current);
        }
        self.events.deliver(Event::FolderListed(listing)).ok();
    }
}

impl Client {
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

pub fn list_folder(
    client: &Client,
    (id, path): (ListingId, PathBuf),
    order: Order,
    hidden: HiddenFiles,
    inside: &Arc<Inside>,
) {
    let job = client.start_listing(id);
    let job_path = path.clone();
    let inside = Arc::clone(inside);
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
                    match (reason, inside::within(&job_path)) {
                        (Unreadable::NotAFolder | Unreadable::Missing, Some(within)) => {
                            let asked = (order, hidden);
                            list_inside(&job, (id, job_path), &within, asked, &inside);
                        }
                        _ => job.answer(Event::FolderUnreadable {
                            id,
                            path: job_path,
                            reason,
                        }),
                    }
                    return;
                }
                Ok(None) | Err(Stopped::Cancelled) => return,
            };
            job.send(Listing {
                id,
                path: job_path,
                entries: Arc::clone(&entries),
                folders: None,
            });
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

fn list_inside(
    job: &ListingJob,
    (id, path): (ListingId, PathBuf),
    within: &Within,
    (order, hidden): (Order, HiddenFiles),
    inside: &Inside,
) {
    let listed = inside
        .tree(&within.archive)
        .and_then(|tree| tree.folder(&within.inner));
    match listed {
        Ok(listed) => {
            let (entries, details) = inside::arrange(listed, order, hidden);
            let listing = Listing {
                id,
                path,
                entries: entries.into(),
                folders: None,
            };
            job.send_known(listing, Some(details.into()));
        }
        Err(reason) => job.answer(Event::FolderUnreadable { id, path, reason }),
    }
}

pub fn open_file(
    client: &Client,
    (types, inside): (Arc<OnceLock<MimeTypes>>, Arc<Inside>),
    path: PathBuf,
    activation: Option<String>,
) {
    let events = Arc::clone(&client.events);
    let failed_path = path.clone();
    let spawned = thread::Builder::new()
        .name(OPEN_THREAD.to_owned())
        .spawn(move || {
            let types = types.get_or_init(MimeTypes::from_env);
            let opened = match opened_copy(&path, &inside) {
                Some(Ok(copy)) => open::open_file(&copy, types, activation.as_deref()),
                Some(Err(_)) => Err(NotOpened::Failed),
                None => open::open_file(&path, types, activation.as_deref()),
            };
            let event = match opened {
                Ok(()) => open_api::Event::Opened { path },
                Err(reason) => open_api::Event::NotOpened { path, reason },
            };
            events.deliver(event).ok();
        });
    if let Err(error) = spawned {
        log::warn!("can't start a thread to open a file: {error}");
        client
            .events
            .deliver(open_api::Event::NotOpened {
                path: failed_path,
                reason: NotOpened::Failed,
            })
            .ok();
    }
}

fn opened_copy(path: &Path, inside: &Inside) -> Option<Result<PathBuf, ArchiveFailure>> {
    if path.symlink_metadata().is_ok() {
        return None;
    }
    inside::within(path).map(|within| inside.take_out(&within))
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    #[test]
    fn a_replaced_listing_answers_nothing() {
        let (sender, events) = mpsc::channel::<Event>();
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
