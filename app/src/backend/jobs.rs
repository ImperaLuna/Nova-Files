use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use nova_files_api::{ClientGone, EntryId, Event, EventSink, FileTypes, ListingId};
use nova_files_disk::ports::Icons;
pub use nova_files_entries_core::ListingJob;
use nova_files_entries_core::{RowPictures, RowWatch};

use nova_files_look_core::FileIcons;
use nova_files_preview_core as preview;
use nova_files_thumbnails_core::{Queue, Thumbnails};

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
        types: Arc<dyn FileTypes>,
    ) -> Self {
        let rows = thumbnails
            .and_then(|setup| Queue::start(setup.clone(), Arc::clone(&events), types))
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
        pictures: Option<RowPictures>,
    ) {
        self.entries.want_rows(id, first, count, pictures);
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
