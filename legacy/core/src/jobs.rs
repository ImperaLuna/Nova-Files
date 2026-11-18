use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;

use nova_files_api::{
    ClientGone, Entry, EntryId, Event, EventSink, HiddenFiles, Listing, ListingId, NotOpened,
    Order, ThumbnailSize, Unreadable,
};
use nova_files_disk::ports::Icons;
pub use nova_files_entries::ListingJob;
use nova_files_entries::RowWatch;

use crate::archive::ArchiveFailure;
use crate::inside::{self, Inside, Within};
use crate::thumbs::{Queue, Thumbnails};
use nova_files_look_core::FileIcons;

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
    entries: nova_files_entries::Client,
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
            entries: nova_files_entries::Client::new(events, rows, &icons),
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

impl RowWatch for Queue {
    fn show(&self, listing: &Listing, current: &dyn Fn() -> bool) {
        Self::show(self, listing, current);
    }

    fn want(&self, id: ListingId, first: EntryId, count: u32, size: Option<ThumbnailSize>) {
        Self::want(self, id, first, count, size);
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
    let events = Arc::clone(client.events());
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
                Ok(()) => Event::FileOpened { path },
                Err(reason) => Event::FileNotOpened { path, reason },
            };
            events.deliver(event).ok();
        });
    if let Err(error) = spawned {
        log::warn!("can't start a thread to open a file: {error}");
        client
            .deliver(Event::FileNotOpened {
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
