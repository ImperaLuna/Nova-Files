use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use nova_files_api::{Details, Entry, Event, HiddenFiles, Listing, ListingId, Order, Unreadable};

use crate::client::{Client, ListingJob};
use crate::list::{self, Stopped};
use crate::{memory, order};

const LIST_THREAD: &str = "nova-files-list";

pub type Inner = Result<(Vec<Entry>, Vec<Details>), Unreadable>;

pub trait InnerFolders: Send + Sync {
    fn list(&self, path: &Path, order: Order, hidden: HiddenFiles) -> Option<Inner>;
}

pub fn list_folder(
    client: &Client,
    (id, path): (ListingId, PathBuf),
    (order, hidden): (Order, HiddenFiles),
    inner: &Arc<dyn InnerFolders>,
) {
    let job = client.start_listing(id);
    let job_path = path.clone();
    let inner = Arc::clone(inner);
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
                    let elsewhere = matches!(reason, Unreadable::NotAFolder | Unreadable::Missing)
                        .then(|| inner.list(&job_path, order, hidden))
                        .flatten();
                    match elsewhere {
                        Some(listed) => send_inner(&job, (id, job_path), listed),
                        None => job.answer(Event::FolderUnreadable {
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

fn send_inner(job: &ListingJob, (id, path): (ListingId, PathBuf), listed: Inner) {
    match listed {
        Ok((entries, details)) => {
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

#[cfg(test)]
mod tests;
