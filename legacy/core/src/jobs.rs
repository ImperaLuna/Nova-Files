use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

use nova_files_api::{
    ClientGone, Entry, EntryId, Event, EventSink, HiddenFiles, Listing, ListingId, Order,
    ThumbnailSize, Unreadable,
};
use nova_files_disk::ports::Icons;
pub use nova_files_entries_core::ListingJob;
use nova_files_entries_core::RowWatch;

use crate::inside::{self, Inside, Within};
use nova_files_look_core::FileIcons;

use crate::list::{self, Stopped};
use crate::memory;
use crate::order;
use nova_files_preview_core as preview;
use nova_files_thumbnails_core::{Queue, Thumbnails};

const LIST_THREAD: &str = "nova-files-list";

const NO_LISTING: u64 = u64::MAX;

pub struct Client {
    entries: nova_files_entries_core::Client,
    current_search: Arc<AtomicU64>,
    current_preview: Arc<preview::Current>,
}

impl Client {
    pub fn new(
        events: Arc<dyn EventSink>,
        thumbnails: Option<&Thumbnails>,
        icons: &Arc<FileIcons>,
    ) -> Self {
        let rows = thumbnails
            .and_then(|setup| Queue::start(setup.clone(), Arc::clone(&events)))
            .map(|queue| Arc::new(queue) as Arc<dyn RowWatch>);
        let icons: Arc<dyn Icons> = Arc::<FileIcons>::clone(icons);
        Self {
            entries: nova_files_entries_core::Client::new(events, rows, &icons),
            current_search: Arc::new(AtomicU64::new(NO_LISTING)),
            current_preview: Arc::new(preview::Current::new()),
        }
    }

    pub fn want_rows(
        &self,
        id: ListingId,
        first: EntryId,
        count: u32,
        thumbnails: Option<ThumbnailSize>,
    ) {
        self.entries.want_rows(id, first, count, thumbnails);
    }

    pub fn deliver(&self, event: Event) -> Result<(), ClientGone> {
        self.entries.deliver(event)
    }

    pub const fn entries(&self) -> &nova_files_entries_core::Client {
        &self.entries
    }

    pub const fn events(&self) -> &Arc<dyn EventSink> {
        self.entries.events()
    }

    pub const fn current_search(&self) -> &Arc<AtomicU64> {
        &self.current_search
    }

    pub const fn current_preview(&self) -> &Arc<preview::Current> {
        &self.current_preview
    }

    pub fn start_listing(&self, id: ListingId) -> ListingJob {
        self.entries.start_listing(id)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.current_search.store(NO_LISTING, Ordering::SeqCst);
        self.current_preview.cancel();
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
