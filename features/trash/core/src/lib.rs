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
            let all = trashes.all();
            let items = all.iter().flat_map(trashes::list).collect();
            let unrecorded = all
                .iter()
                .map(|trash| trashes::unrecorded(trash).len())
                .sum();
            let folder = trashes.files();
            let listed = Event::Listed {
                folder,
                items,
                unrecorded,
            };
            thread_events.deliver(listed).ok();
        });
    if spawned.is_err() {
        return events.deliver(Event::Listed {
            folder: None,
            items: Vec::new(),
            unrecorded: 0,
        });
    }
    Ok(())
}
