use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use nova_files_api::{Details, Entry, EntryId, EntryKind, Event, Listing, ListingId, Order};

use crate::client::ListingJob;
use crate::{details, order};

pub const FRESH: Duration = Duration::from_secs(10);

const KEPT_FOLDERS: usize = 16;

pub type Told<'a> = &'a (dyn Fn(Vec<(usize, u64)>) + Sync);

pub trait MeasureFolders: Send + Sync {
    fn measure(
        &self,
        parent: &Path,
        folders: &[OsString],
        stopped: &(dyn Fn() -> bool + Sync),
        told: Told<'_>,
    ) -> Option<Vec<u64>>;
}

type Sizes = Arc<HashMap<OsString, u64>>;

struct Kept {
    at: Instant,
    sizes: Sizes,
}

pub struct FolderSizer {
    measure: Arc<dyn MeasureFolders>,
    kept: Mutex<HashMap<PathBuf, Kept>>,
}

impl FolderSizer {
    pub fn new(measure: Arc<dyn MeasureFolders>) -> Self {
        Self {
            measure,
            kept: Mutex::new(HashMap::new()),
        }
    }

    fn known(&self, folder: &Path) -> Option<(Sizes, bool)> {
        self.lock()
            .get(folder)
            .map(|kept| (Arc::clone(&kept.sizes), kept.at.elapsed() < FRESH))
    }

    fn keep(&self, folder: PathBuf, sizes: HashMap<OsString, u64>) {
        let mut kept = self.lock();
        if kept.len() >= KEPT_FOLDERS && !kept.contains_key(&folder) {
            let oldest = kept
                .iter()
                .min_by_key(|(_, kept)| kept.at)
                .map(|(folder, _)| folder.clone());
            if let Some(oldest) = oldest {
                kept.remove(&oldest);
            }
        }
        kept.insert(
            folder,
            Kept {
                at: Instant::now(),
                sizes: Arc::new(sizes),
            },
        );
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<PathBuf, Kept>> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub fn list_measured(
    job: &ListingJob,
    (id, path): (ListingId, PathBuf),
    entries: Vec<Entry>,
    order: Order,
    sizer: &FolderSizer,
) {
    let cancelled = || job.cancelled();
    let known = sizer.known(&path);
    let fresh = known
        .as_ref()
        .is_some_and(|(kept, fresh)| *fresh && every_folder_in(&entries, kept));
    let listed = match &known {
        Some((kept, _)) => {
            arrange(&path, entries, order.descending, kept, cancelled).map(|(entries, known)| {
                let listing = listing(id, path.clone(), entries);
                job.send_known(listing.clone(), Some(known.into()));
                listing
            })
        }
        None => order::arrange(&path, entries, order, cancelled).map(|entries| {
            let listing = listing(id, path.clone(), entries);
            job.send(listing.clone());
            listing
        }),
    };
    if let Some(listing) = listed.filter(|_| !fresh) {
        measure(job, &listing, sizer);
    }
}

fn every_folder_in(entries: &[Entry], kept: &HashMap<OsString, u64>) -> bool {
    entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::Folder)
        .all(|entry| kept.contains_key(&entry.name))
}

fn listing(id: ListingId, path: PathBuf, entries: Vec<Entry>) -> Listing {
    Listing {
        id,
        path,
        entries: entries.into(),
        folders: None,
    }
}

fn measure(job: &ListingJob, listing: &Listing, sizer: &FolderSizer) {
    let folders: Vec<&Entry> = listing
        .entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::Folder)
        .collect();
    if folders.is_empty() {
        return;
    }
    let id = listing.id;
    let by_slot = |sizes: Vec<(usize, u64)>| -> Vec<(EntryId, u64)> {
        sizes
            .into_iter()
            .filter_map(|(slot, bytes)| folders.get(slot).map(|entry| (entry.id, bytes)))
            .collect()
    };
    let told = |sizes: Vec<(usize, u64)>| {
        job.answer(Event::FoldersMeasured {
            id,
            sizes: by_slot(sizes),
            done: false,
        });
    };
    let names: Vec<OsString> = folders.iter().map(|entry| entry.name.clone()).collect();
    let stopped = || job.cancelled();
    let Some(totals) = sizer
        .measure
        .measure(&listing.path, &names, &stopped, &told)
        .filter(|_| !job.cancelled())
    else {
        return;
    };
    sizer.keep(
        listing.path.clone(),
        names.into_iter().zip(totals.iter().copied()).collect(),
    );
    job.answer(Event::FoldersMeasured {
        id,
        sizes: by_slot(totals.into_iter().enumerate().collect()),
        done: true,
    });
}

fn arrange(
    folder: &Path,
    entries: Vec<Entry>,
    descending: bool,
    sizes: &HashMap<OsString, u64>,
    cancelled: impl Fn() -> bool,
) -> Option<(Vec<Entry>, Vec<Details>)> {
    let paths = entries
        .iter()
        .map(|entry| (folder.join(&entry.name), entry.kind));
    let read = details::read_batch(paths, cancelled)?;
    let mut paired: Vec<(Entry, Details)> = entries
        .into_iter()
        .zip(read)
        .map(|(entry, mut details)| {
            if entry.kind == EntryKind::Folder {
                details.size = sizes.get(&entry.name).copied();
            }
            (entry, details)
        })
        .collect();
    paired.sort_by(|(_, left), (_, right)| order::known_first(left.size, right.size, descending));
    Some(
        paired
            .into_iter()
            .zip(0..)
            .map(|((entry, details), index)| {
                (
                    Entry {
                        id: EntryId(index),
                        ..entry
                    },
                    details,
                )
            })
            .unzip(),
    )
}

#[cfg(test)]
mod tests;
