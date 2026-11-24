use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use nova_files_api::{
    ClientGone, Details, EntryId, Event, EventSink, Listing, ListingId, Message, ThumbnailSize,
};
use nova_files_disk::ports::Icons;

use crate::details_queue;

const NO_LISTING: u64 = u64::MAX;

pub trait RowWatch: Send + Sync {
    fn show(&self, listing: &Listing, current: &dyn Fn() -> bool);
    fn want(&self, id: ListingId, first: EntryId, count: u32, pictures: Option<RowPictures>);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowPictures {
    pub size: ThumbnailSize,
    pub peeks: bool,
}

pub struct Client {
    events: Arc<dyn EventSink>,
    current_listing: Arc<AtomicU64>,
    details: Option<Arc<details_queue::Queue>>,
    rows: Option<Arc<dyn RowWatch>>,
    icons: Arc<dyn Icons>,
}

impl Client {
    pub fn new(
        events: Arc<dyn EventSink>,
        rows: Option<Arc<dyn RowWatch>>,
        icons: &Arc<dyn Icons>,
    ) -> Self {
        let details =
            details_queue::Queue::start(Arc::clone(&events), Arc::clone(icons)).map(Arc::new);
        Self {
            events,
            current_listing: Arc::new(AtomicU64::new(NO_LISTING)),
            details,
            rows,
            icons: Arc::clone(icons),
        }
    }

    pub fn want_rows(
        &self,
        id: ListingId,
        first: EntryId,
        count: u32,
        pictures: Option<RowPictures>,
    ) {
        if let Some(queue) = &self.details {
            queue.want(id, first, count);
        }
        if let Some(rows) = &self.rows {
            rows.want(id, first, pictures.map_or(0, |_| count), pictures);
        }
    }

    pub fn deliver(&self, event: Event) -> Result<(), ClientGone> {
        self.events.deliver(event)
    }

    pub const fn events(&self) -> &Arc<dyn EventSink> {
        &self.events
    }

    pub fn start_listing(&self, id: ListingId) -> ListingJob {
        self.current_listing.store(id.0, Ordering::SeqCst);
        ListingJob {
            id,
            events: Arc::clone(&self.events),
            current: Arc::clone(&self.current_listing),
            details: self.details.clone(),
            rows: self.rows.clone(),
            icons: Arc::clone(&self.icons),
        }
    }

    fn cancel_listing(&self) {
        self.current_listing.store(NO_LISTING, Ordering::SeqCst);
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.cancel_listing();
    }
}

pub struct ListingJob {
    id: ListingId,
    events: Arc<dyn EventSink>,
    current: Arc<AtomicU64>,
    details: Option<Arc<details_queue::Queue>>,
    rows: Option<Arc<dyn RowWatch>>,
    icons: Arc<dyn Icons>,
}

impl ListingJob {
    pub fn kinds(&self) -> &dyn Icons {
        self.icons.as_ref()
    }

    pub fn cancelled(&self) -> bool {
        self.current.load(Ordering::SeqCst) != self.id.0
    }

    pub fn answer<M: Message>(&self, message: M) {
        if !self.cancelled() {
            self.events.deliver(message).ok();
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
        if let Some(rows) = &self.rows {
            rows.show(&listing, &current);
        }
        self.events.deliver(Event::FolderListed(listing)).ok();
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::mpsc;

    use nova_files_api::{EntryKind, FileIcon, IconKind};

    use super::*;

    struct Plain;

    impl Icons for Plain {
        fn of(&self, _: &std::ffi::OsStr, _: EntryKind) -> FileIcon {
            FileIcon::BuiltIn(IconKind::Folder)
        }

        fn kind_name(&self, _: &std::ffi::OsStr, _: EntryKind) -> Option<Arc<str>> {
            None
        }

        fn follow_desktop(&self, _: &[String]) {}
    }

    #[test]
    fn a_replaced_listing_answers_nothing() {
        let (sender, events) = mpsc::channel::<Event>();
        let icons: Arc<dyn Icons> = Arc::new(Plain);
        let client = Client::new(Arc::new(sender), None, &icons);
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
