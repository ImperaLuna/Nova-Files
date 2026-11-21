use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use nova_files_api::{Details, Entry, EntryId, EntryKind, Event, Listing, ListingId, Order};

use crate::client::ListingJob;
use crate::order;

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

    fn known(&self, folder: &Path) -> Option<Sizes> {
        self.lock().get(folder).map(|kept| Arc::clone(&kept.sizes))
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

type ByHome = HashMap<PathBuf, Sizes>;

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
    let listed = if kept.is_empty() {
        order::arrange_paired(rows, |entry, home| home.join(&entry.name), order, cancelled).map(
            |rows| {
                let listing = listing(id, path, rows, &homes);
                job.send(listing.clone());
                listing
            },
        )
    } else {
        send_sorted(job, (id, path), rows, (order.descending, &homes), &kept)
    };
    let Some(listing) = listed else {
        return;
    };
    let Some(measured) = measure(job, &listing, sizer) else {
        return;
    };
    let rows = rows_of(&listing);
    let place = (id, listing.path);
    if let Some(sorted) = send_sorted(job, place, rows, (order.descending, &homes), &measured) {
        job.answer(Event::FoldersMeasured {
            id,
            sizes: every_total(&sorted, &measured),
        });
    }
}

fn every_total(listing: &Listing, measured: &ByHome) -> Vec<(EntryId, u64)> {
    listing
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.kind == EntryKind::Folder)
        .filter_map(|(index, entry)| {
            let home = home_of(listing, index);
            let bytes = measured.get(home)?.get(&entry.name)?;
            Some((entry.id, *bytes))
        })
        .collect()
}

fn send_sorted(
    job: &ListingJob,
    (id, path): (ListingId, PathBuf),
    rows: Vec<Row>,
    (descending, homes): (bool, &Homes),
    sizes: &ByHome,
) -> Option<Listing> {
    let size_of = |entry: &Entry, home: &PathBuf| {
        sizes
            .get(home)
            .and_then(|sizes| sizes.get(&entry.name))
            .copied()
    };
    let (rows, known) = arrange(rows, descending, size_of, || job.cancelled())?;
    let listing = listing(id, path, rows, homes);
    job.send_known(listing.clone(), Some(known.into()));
    Some(listing)
}

impl FolderSizer {
    fn kept_for(&self, rows: &[Row]) -> ByHome {
        let mut kept = ByHome::new();
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

fn rows_of(listing: &Listing) -> Vec<Row> {
    listing
        .entries
        .iter()
        .enumerate()
        .map(|(index, entry)| (entry.clone(), home_of(listing, index).to_path_buf()))
        .collect()
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

fn measure(job: &ListingJob, listing: &Listing, sizer: &FolderSizer) -> Option<ByHome> {
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
        return None;
    }
    let mut measured = Vec::new();
    for (home, folders) in &groups {
        let totals = measure_in(job, listing.id, home, folders, sizer)?;
        measured.push((home.to_path_buf(), totals));
    }
    let by_home = measured
        .iter()
        .map(|(home, totals)| (home.clone(), Arc::new(totals.clone())))
        .collect();
    sizer.keep(measured);
    Some(by_home)
}

fn measure_in(
    job: &ListingJob,
    id: ListingId,
    home: &Path,
    folders: &[&Entry],
    sizer: &FolderSizer,
) -> Option<ByName> {
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
        });
    };
    let names: Vec<OsString> = folders.iter().map(|entry| entry.name.clone()).collect();
    let stopped = || job.cancelled();
    let totals = sizer
        .measure
        .measure(home, &names, &stopped, &told)
        .filter(|_| !job.cancelled())?;
    Some(names.into_iter().zip(totals).collect())
}

fn arrange(
    rows: Vec<Row>,
    descending: bool,
    size_of: impl Fn(&Entry, &PathBuf) -> Option<u64>,
    cancelled: impl Fn() -> bool,
) -> Option<(Vec<Row>, Vec<Details>)> {
    let sorted = order::sorted_with_details(
        rows,
        |entry, home| home.join(&entry.name),
        |entry, home, details| {
            if entry.kind == EntryKind::Folder {
                details.size = size_of(entry, home);
            }
        },
        |(_, left), (_, right)| order::known_first(left.size, right.size, descending),
        cancelled,
    )?;
    Some(sorted.into_iter().unzip())
}

#[cfg(test)]
mod tests;
