#![forbid(unsafe_code)]

mod kind;
mod restoring;
mod trashes;
mod trashing;

use std::sync::Arc;
use std::thread;

use nova_files_messages::{ClientGone, EventSink};
use nova_files_trash_api::Event;

pub use kind::{TrashJobs, Undo};
pub use restoring::{delete_from_trash, empty_trash, restore};
pub use trashes::{Trashes, find, trash_of};
pub use trashing::{Trasher, trash};

const LIST_THREAD: &str = "nova-files-trash";

pub fn list(trashes: Trashes, events: &Arc<dyn EventSink>) -> Result<(), ClientGone> {
    let thread_events = Arc::clone(events);
    let spawned = thread::Builder::new()
        .name(LIST_THREAD.to_owned())
        .spawn(move || {
            let items = trashes.all().iter().flat_map(trashes::list).collect();
            let folder = trashes.files();
            thread_events.deliver(Event::Listed { folder, items }).ok();
        });
    if spawned.is_err() {
        return events.deliver(Event::Listed {
            folder: None,
            items: Vec::new(),
        });
    }
    Ok(())
}
