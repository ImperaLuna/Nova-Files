//! The jobs that take entries out of the trash: restore, delete from the
//! trash, and empty the trash.
//!
//! Each entry is named by where it sits in a trash's `files/`, and must
//! be one the trash lists (see `crate::trash`): an entry with an info
//! file that reads. Before any entry leaves, its info file is recorded
//! in the journal with its device and inode, and the entry's; once the
//! entry is gone, the info file goes. After a crash, an info file still
//! the job's own whose entry has left goes too (see `left`), so the
//! trash never lists what isn't there. An entry skipped, or a folder
//! only partly deleted, keeps its info file and stays listed.
//!
//! A restore moves each entry back where its info file says, making the
//! folders it was in when they are gone; what stands in the way there
//! is a conflict, as for a move. Deleting from the trash is a permanent
//! delete. Emptying the trash deletes every entry every trash lists.
//!
//! A trashed folder's line in its trash's `directorysizes` stays: other
//! apps check it against the info file, which is gone.

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use nova_files_api::{Failure, Outcome};

use super::Ended;
use super::delete::delete_paths;
use super::journal::{Record, word};
use super::left::remove_lone_info;
use super::make::How;
use super::plan::Entry;
use super::running::Running;
use super::stop::Stop;
use super::transfer::transfer_to;
use crate::trash::{self, TrashFolder, Trashes};

/// An entry on its way out of a trash: where it sits, its info file and
/// which entry each is, and where it was trashed from.
struct Leaving {
    item: PathBuf,
    info: PathBuf,
    info_entry: Entry,
    item_entry: Entry,
    original: PathBuf,
    trash: TrashFolder,
}

/// Puts each of `items` back where it was trashed from.
pub fn restore(running: &mut Running, items: &[PathBuf], trashes: &Trashes) -> Ended {
    record(running, word::RESTORE, items);
    let leaving = match leaving(items, trashes) {
        Ok(leaving) => leaving,
        Err(stop) => return (stop.outcome(), Vec::new()),
    };
    if let Err(stop) = make_folders(&leaving) {
        return (stop.outcome(), Vec::new());
    }
    note_infos(running, &leaving);
    let pairs: Vec<_> = leaving
        .iter()
        .map(|left| (left.item.clone(), left.original.clone()))
        .collect();
    let (result, mut touched) = transfer_to(running, &pairs, How::Move);
    touched.extend(drop_infos(&leaving));
    // Only the folders the user sees: where the entries went, and the
    // trash.
    let shown = leaving
        .iter()
        .filter_map(|left| left.original.parent())
        .chain(leaving.iter().map(|left| left.trash.root.as_path()))
        .filter(|folder| touched.iter().any(|touched| touched.starts_with(folder)))
        .map(Path::to_path_buf)
        .collect::<BTreeSet<_>>();
    ended(result, shown)
}

/// Deletes each of `items` from its trash for good.
pub fn delete_from_trash(running: &mut Running, items: &[PathBuf], trashes: &Trashes) -> Ended {
    record(running, word::DELETE_FROM_TRASH, items);
    delete_leaving(running, leaving(items, trashes))
}

/// Deletes every entry in every trash for good.
pub fn empty_trash(running: &mut Running, trashes: &Trashes) -> Ended {
    record(running, word::EMPTY_TRASH, &[]);
    let trashes = trashes.all();
    let items: Vec<_> = trashes
        .iter()
        .flat_map(trash::list)
        .map(|listed| listed.item)
        .collect();
    delete_leaving(running, leaving_from(&items, &trashes))
}

/// Records operation `name` and its `sources`.
fn record(running: &mut Running, name: &'static str, sources: &[PathBuf]) {
    running.record(Record::Operation(name));
    for source in sources {
        running.record(Record::Source(source));
    }
    running.record(Record::SourcesDone);
}

fn delete_leaving(running: &mut Running, leaving: Result<Vec<Leaving>, Stop>) -> Ended {
    let leaving = match leaving {
        Ok(leaving) => leaving,
        Err(stop) => return (stop.outcome(), Vec::new()),
    };
    note_infos(running, &leaving);
    let items: Vec<_> = leaving.iter().map(|left| left.item.clone()).collect();
    let (result, _) = delete_paths(running, &items);
    let touched = drop_infos(&leaving);
    let shown = leaving
        .iter()
        .map(|left| left.trash.root.clone())
        .filter(|root| touched.iter().any(|touched| touched.starts_with(root)))
        .collect::<BTreeSet<_>>();
    ended(result, shown)
}

fn ended(result: Result<(), Stop>, shown: BTreeSet<PathBuf>) -> Ended {
    (
        result.map_or_else(Stop::outcome, |()| Outcome::Done),
        shown.into_iter().collect(),
    )
}

/// Each of `items` as an entry leaving its trash.
///
/// # Errors
/// [`Stop`] when one can't be looked at, or isn't an entry a trash
/// lists, so nothing is changed.
fn leaving(items: &[PathBuf], trashes: &Trashes) -> Result<Vec<Leaving>, Stop> {
    leaving_from(items, &trashes.all())
}

fn leaving_from(items: &[PathBuf], trashes: &[TrashFolder]) -> Result<Vec<Leaving>, Stop> {
    let mut seen = HashSet::new();
    items
        .iter()
        .filter(|item| seen.insert(*item))
        .map(|item| {
            let there = item
                .symlink_metadata()
                .map_err(|error| Stop::at(item, &error))?;
            let not_trashed = || Stop::because(item, Failure::Unsupported);
            let (trash, listed) = trash::find(item, trashes).ok_or_else(not_trashed)?;
            let info = trash::info_of(&trash, item.file_name().ok_or_else(not_trashed)?);
            let info_there = info
                .symlink_metadata()
                .map_err(|error| Stop::at(&info, &error))?;
            Ok(Leaving {
                item: item.clone(),
                info,
                info_entry: Entry::of(&info_there),
                item_entry: Entry::of(&there),
                original: listed.original,
                trash,
            })
        })
        .collect()
}

/// Makes the folders each restored entry was in, when they are gone.
///
/// # Errors
/// [`Stop`] when one can't be made.
fn make_folders(leaving: &[Leaving]) -> Result<(), Stop> {
    for parent in leaving.iter().filter_map(|left| left.original.parent()) {
        fs::create_dir_all(parent).map_err(|error| Stop::at(parent, &error))?;
    }
    Ok(())
}

/// Records each info file of `leaving`, on the disk before any entry
/// leaves.
fn note_infos(running: &mut Running, leaving: &[Leaving]) {
    for left in leaving {
        running.record(Record::TrashInfo(
            &left.info,
            left.info_entry,
            left.item_entry,
        ));
    }
    running.settle();
}

/// Removes the info file of each entry of `leaving` that has left, and
/// returns the trash folders that changed.
fn drop_infos(leaving: &[Leaving]) -> BTreeSet<PathBuf> {
    let mut touched = BTreeSet::new();
    for left in leaving {
        remove_lone_info(&left.info, left.info_entry, left.item_entry);
        if left.info.symlink_metadata().is_err() {
            touched.insert(left.trash.info());
            touched.insert(left.trash.files());
        }
    }
    touched
}
