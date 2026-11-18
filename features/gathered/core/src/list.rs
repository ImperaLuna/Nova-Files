use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use nova_files_api::{
    Entry, EntryId, EntryKind, Event, HiddenFiles, Listing, ListingId, Order, Unreadable,
};
use nova_files_entries_core::list::{self, Stopped};
use nova_files_entries_core::{Client, memory, order};
use nova_files_gathered_api::{GatheredFolder, Holds};

const THREAD: &str = "nova-files-list-gathered";

pub fn list(
    client: &Client,
    id: ListingId,
    folders: impl FnOnce() -> Arc<[GatheredFolder]> + Send + 'static,
    order: Order,
    hidden: HiddenFiles,
) {
    let job = client.start_listing(id);
    let spawned = thread::Builder::new()
        .name(THREAD.to_owned())
        .spawn(move || {
            if job.cancelled() {
                return;
            }
            let folders = folders();
            let Some(gathered) = gather(&folders, hidden, order, || job.cancelled()) else {
                return;
            };
            let (entries, homes): (Vec<Entry>, Vec<PathBuf>) = gathered.into_iter().unzip();
            job.send(Listing {
                id,
                path: PathBuf::new(),
                entries: entries.into(),
                folders: Some(homes.into()),
            });
            memory::give_back();
        });
    if spawned.is_err() {
        client
            .deliver(Event::FolderUnreadable {
                id,
                path: PathBuf::new(),
                reason: Unreadable::Failed,
            })
            .ok();
    }
}

pub fn gather(
    folders: &[GatheredFolder],
    hidden: HiddenFiles,
    order: Order,
    cancelled: impl Fn() -> bool,
) -> Option<Vec<(Entry, PathBuf)>> {
    let mut found: Vec<(Entry, PathBuf)> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for folder in folders {
        if cancelled() {
            return None;
        }
        for (name, home) in rows_of(folder, hidden, &cancelled)? {
            if seen.insert(home.join(&name)) {
                found.push((
                    Entry {
                        id: EntryId(0),
                        name,
                        kind: EntryKind::Folder,
                    },
                    home,
                ));
            }
        }
    }
    found.sort_by_cached_key(|(entry, _)| list::sort_key(&(entry.name.clone(), entry.kind)));
    order::arrange_paired(
        found,
        |entry, home| home.join(&entry.name),
        order,
        cancelled,
    )
}

fn rows_of(
    folder: &GatheredFolder,
    hidden: HiddenFiles,
    cancelled: &impl Fn() -> bool,
) -> Option<Vec<(std::ffi::OsString, PathBuf)>> {
    match folder.holds {
        Holds::One => {
            let (Some(name), Some(home)) = (folder.path.file_name(), folder.path.parent()) else {
                return Some(Vec::new());
            };
            let is_folder = fs::metadata(&folder.path).is_ok_and(|found| found.is_dir());
            let entry = Entry {
                id: EntryId(0),
                name: name.to_owned(),
                kind: EntryKind::Folder,
            };
            let shown = hidden == HiddenFiles::Shown || !entry.is_hidden();
            Some(if is_folder && shown {
                vec![(name.to_owned(), home.to_owned())]
            } else {
                Vec::new()
            })
        }
        Holds::Many => match list::read_folder(&folder.path, cancelled) {
            Ok(entries) => Some(
                list::keep_shown(entries, hidden)
                    .into_iter()
                    .filter(|entry| entry.kind == EntryKind::Folder)
                    .map(|entry| (entry.name, folder.path.clone()))
                    .collect(),
            ),
            Err(Stopped::Cancelled) => None,
            Err(Stopped::Unreadable(_)) => Some(Vec::new()),
        },
    }
}

#[cfg(test)]
mod tests;
