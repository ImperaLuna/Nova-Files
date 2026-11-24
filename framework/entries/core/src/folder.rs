use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use nova_files_api::{
    Details, Entry, Event, HiddenFiles, Listing, ListingId, Order, SortKey, Unreadable,
};

use crate::client::{Client, ListingJob};
use crate::list::{self, Stopped};
use crate::sizes::{self, FolderSizer};
use crate::{memory, order};

const LIST_THREAD: &str = "nova-files-list";

pub type Inner = Result<(Vec<Entry>, Vec<Details>), Unreadable>;

pub trait InnerFolders: Send + Sync {
    fn list(&self, path: &Path, order: Order, hidden: HiddenFiles) -> Option<Inner>;
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Shown {
    pub order: Order,
    pub hidden: HiddenFiles,
    pub filter: String,
}

pub fn list_folder(
    client: &Client,
    (id, path): (ListingId, PathBuf),
    shown: Shown,
    inner: &Arc<dyn InnerFolders>,
    sizer: Option<&Arc<FolderSizer>>,
) {
    let Shown {
        order,
        hidden,
        filter,
    } = shown;
    let job = client.start_listing(id);
    let job_path = path.clone();
    let inner = Arc::clone(inner);
    let sizer = sizer.filter(|_| order.key == SortKey::Size).cloned();
    let spawned = thread::Builder::new()
        .name(LIST_THREAD.to_owned())
        .spawn(move || {
            let cancelled = || job.cancelled();
            let entries = match list::read_folder(&job_path, cancelled) {
                Ok(entries) => list::keep_named(list::keep_shown(entries, hidden), &filter),
                Err(Stopped::Unreadable(reason)) => {
                    let elsewhere = matches!(reason, Unreadable::NotAFolder | Unreadable::Missing)
                        .then(|| inner.list(&job_path, order, hidden))
                        .flatten()
                        .map(|listed| list::keep_named_known(listed, &filter));
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
                Err(Stopped::Cancelled) => return,
            };
            if let Some(sizer) = sizer {
                sizes::list_measured(&job, (id, job_path), entries, order, &sizer);
            } else if let Some(entries) = order::arrange(&job_path, entries, order, cancelled) {
                job.send(Listing {
                    id,
                    path: job_path,
                    entries: entries.into(),
                    folders: None,
                });
            }
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
