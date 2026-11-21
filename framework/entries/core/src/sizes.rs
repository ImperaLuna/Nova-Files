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

type ByName = HashMap<OsString, u64>;

type Sizes = Arc<ByName>;

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

    fn keep(&self, measured: Vec<(PathBuf, ByName)>) {
        let at = Instant::now();
        let room = KEPT_FOLDERS.max(measured.len());
        let fresh: Vec<PathBuf> = measured.iter().map(|(folder, _)| folder.clone()).collect();
        let mut kept = self.lock();
        for (folder, sizes) in measured {
            kept.insert(
                folder,
                Kept {
                    at,
                    sizes: Arc::new(sizes),
                },
            );
        }
        while kept.len() > room {
            let oldest = kept
                .iter()
                .filter(|(folder, _)| !fresh.contains(folder))
                .min_by_key(|(_, kept)| kept.at)
                .map(|(folder, _)| folder.clone());
            let Some(oldest) = oldest else {
                break;
            };
            kept.remove(&oldest);
        }
        drop(kept);
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<PathBuf, Kept>> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub enum Homes {
    Shared,
    PerRow,
}

type Row = (Entry, PathBuf);

type KeptFor = HashMap<PathBuf, (Sizes, bool)>;

pub fn list_measured(
    job: &ListingJob,
    (id, path): (ListingId, PathBuf),
    entries: Vec<Entry>,
    order: Order,
    sizer: &FolderSizer,
) {
    let rows = entries
        .into_iter()
        .map(|entry| (entry, path.clone()))
        .collect();
    list_measured_rows(job, (id, path), rows, (order, Homes::Shared), sizer);
}

pub fn list_measured_rows(
    job: &ListingJob,
    (id, path): (ListingId, PathBuf),
    rows: Vec<Row>,
    (order, homes): (Order, Homes),
    sizer: &FolderSizer,
) {
    let cancelled = || job.cancelled();
    let kept = sizer.kept_for(&rows);
    let fresh = every_folder_fresh(&rows, &kept);
    let listed = if kept.is_empty() {
        order::arrange_paired(rows, |entry, home| home.join(&entry.name), order, cancelled).map(
            |rows| {
                let listing = listing(id, path, rows, &homes);
                job.send(listing.clone());
                listing
            },
        )
    } else {
        arrange(rows, order.descending, &kept, cancelled).map(|(rows, known)| {
            let listing = listing(id, path, rows, &homes);
            job.send_known(listing.clone(), Some(known.into()));
            listing
        })
    };
    if let Some(listing) = listed.filter(|_| !fresh) {
        measure(job, &listing, sizer);
    }
}

impl FolderSizer {
    fn kept_for(&self, rows: &[Row]) -> KeptFor {
        let mut kept = KeptFor::new();
        for (_, home) in rows
            .iter()
            .filter(|(entry, _)| entry.kind == EntryKind::Folder)
        {
            if !kept.contains_key(home)
                && let Some(known) = self.known(home)
            {
                kept.insert(home.clone(), known);
            }
        }
        kept
    }
}

fn every_folder_fresh(rows: &[Row], kept: &KeptFor) -> bool {
    rows.iter()
        .filter(|(entry, _)| entry.kind == EntryKind::Folder)
        .all(|(entry, home)| {
            kept.get(home)
                .is_some_and(|(sizes, fresh)| *fresh && sizes.contains_key(&entry.name))
        })
}

fn listing(id: ListingId, path: PathBuf, rows: Vec<Row>, homes: &Homes) -> Listing {
    let (entries, folders): (Vec<Entry>, Vec<PathBuf>) = rows.into_iter().unzip();
    Listing {
        id,
        path,
        entries: entries.into(),
        folders: matches!(homes, Homes::PerRow).then(|| folders.into()),
    }
}

fn home_of(listing: &Listing, index: usize) -> &Path {
    listing
        .folders
        .as_ref()
        .and_then(|folders| folders.get(index))
        .map_or(listing.path.as_path(), PathBuf::as_path)
}

fn measure(job: &ListingJob, listing: &Listing, sizer: &FolderSizer) {
    let mut groups: Vec<(&Path, Vec<&Entry>)> = Vec::new();
    for (index, entry) in listing.entries.iter().enumerate() {
        if entry.kind != EntryKind::Folder {
            continue;
        }
        let home = home_of(listing, index);
        match groups.iter_mut().find(|(known, _)| *known == home) {
            Some((_, folders)) => folders.push(entry),
            None => groups.push((home, vec![entry])),
        }
    }
    if groups.is_empty() {
        return;
    }
    let mut measured = Vec::new();
    let mut to_keep = Vec::new();
    for (home, folders) in &groups {
        let Some((in_home, totals)) = measure_in(job, listing.id, home, folders, sizer) else {
            return;
        };
        measured.extend(in_home);
        to_keep.push((home.to_path_buf(), totals));
    }
    sizer.keep(to_keep);
    job.answer(Event::FoldersMeasured {
        id: listing.id,
        sizes: measured,
        done: true,
    });
}

fn measure_in(
    job: &ListingJob,
    id: ListingId,
    home: &Path,
    folders: &[&Entry],
    sizer: &FolderSizer,
) -> Option<(Vec<(EntryId, u64)>, ByName)> {
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
    let totals = sizer
        .measure
        .measure(home, &names, &stopped, &told)
        .filter(|_| !job.cancelled())?;
    let by_name = names.into_iter().zip(totals.iter().copied()).collect();
    Some((by_slot(totals.into_iter().enumerate().collect()), by_name))
}

fn arrange(
    rows: Vec<Row>,
    descending: bool,
    kept: &KeptFor,
    cancelled: impl Fn() -> bool,
) -> Option<(Vec<Row>, Vec<Details>)> {
    let paths = rows
        .iter()
        .map(|(entry, home)| (home.join(&entry.name), entry.kind));
    let read = details::read_batch(paths, cancelled)?;
    let mut paired: Vec<(Row, Details)> = rows
        .into_iter()
        .zip(read)
        .map(|((entry, home), mut details)| {
            if entry.kind == EntryKind::Folder {
                details.size = kept
                    .get(&home)
                    .and_then(|(sizes, _)| sizes.get(&entry.name))
                    .copied();
            }
            ((entry, home), details)
        })
        .collect();
    paired.sort_by(|(_, left), (_, right)| order::known_first(left.size, right.size, descending));
    Some(
        paired
            .into_iter()
            .zip(0..)
            .map(|(((entry, home), details), index)| {
                (
                    (
                        Entry {
                            id: EntryId(index),
                            ..entry
                        },
                        home,
                    ),
                    details,
                )
            })
            .unzip(),
    )
}

#[cfg(test)]
mod tests;
