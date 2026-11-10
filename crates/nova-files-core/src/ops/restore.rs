//! The jobs that take entries out of the trash: restore, delete from the
//! trash, and empty the trash.
//!
//! Each entry is named by where it sits in a trash's `files/`, and must
//! be one that trash lists (see `crate::trash`): an entry with an info
//! file that reads, in the home trash or a drive's, found by its path
//! alone. Before any entry leaves, its info file is recorded in the
//! journal with its device and inode, and the entry's; once the entry is
//! gone, the info file goes. After a crash, an info file still the job's
//! own whose entry has left goes too (see `left`), so the trash never
//! lists what isn't there, and a resumed job leaves alone whatever took a
//! name it freed. An entry skipped, or a folder only partly deleted,
//! keeps its info file and stays listed.
//!
//! A restore moves each entry back where its info file says, shallowest
//! first, making the folders it was in when they are gone, unless
//! another entry restored with it is that folder; what stands in the way
//! there is a conflict, as for a move. Deleting from the trash is a
//! permanent delete. Emptying the trash deletes every entry the trashes
//! list; one that leaves meanwhile is left out.
//!
//! A trashed folder's line in its trash's `directorysizes` stays: other
//! apps check it against the info file, which is gone.

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use nova_files_api::{Failure, Outcome, Trashed};

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
    running.record_job(word::RESTORE, None, items);
    let mut leaving = match leaving(running, items, trashes) {
        Ok(leaving) => leaving,
        Err(stop) => return (stop.outcome(), Vec::new()),
    };
    // A folder goes back before what was in it.
    leaving.sort_by_key(|left| left.original.components().count());
    let mut touched = BTreeSet::new();
    if let Err(stop) = make_folders(running, &leaving, &mut touched) {
        return (stop.outcome(), touched.into_iter().collect());
    }
    note_infos(running, &leaving);
    let pairs: Vec<_> = leaving
        .iter()
        .map(|left| (left.item.clone(), left.original.clone()))
        .collect();
    let (result, moved) = transfer_to(running, &pairs, How::Move);
    let dropped = drop_infos(&leaving);
    // Only the folders the user sees: where the entries went, and the
    // trash.
    let shown = leaving
        .iter()
        .filter_map(|left| left.original.parent())
        .chain(leaving.iter().map(|left| left.trash.root.as_path()))
        .filter(|folder| {
            moved
                .iter()
                .chain(&dropped)
                .any(|changed| changed.starts_with(folder))
        })
        .map(Path::to_path_buf);
    touched.extend(shown);
    ended(result, touched)
}

/// Deletes each of `items` from its trash for good.
pub fn delete_from_trash(running: &mut Running, items: &[PathBuf], trashes: &Trashes) -> Ended {
    running.record_job(word::DELETE_FROM_TRASH, None, items);
    match leaving(running, items, trashes) {
        Ok(leaving) => delete_leaving(running, &leaving),
        Err(stop) => (stop.outcome(), Vec::new()),
    }
}

/// Deletes every entry in every trash for good.
pub fn empty_trash(running: &mut Running, trashes: &Trashes) -> Ended {
    running.record_job(word::EMPTY_TRASH, None, &[]);
    let leaving: Vec<_> = trashes
        .all()
        .into_iter()
        .flat_map(|trash| {
            trash::list(&trash)
                .into_iter()
                .map(move |listed| (trash.clone(), listed))
        })
        // Gone meanwhile: as the user wanted.
        .filter_map(|(trash, listed)| leave(running, trash, listed).ok().flatten())
        .collect();
    delete_leaving(running, &leaving)
}

fn delete_leaving(running: &mut Running, leaving: &[Leaving]) -> Ended {
    note_infos(running, leaving);
    let items: Vec<_> = leaving.iter().map(|left| left.item.clone()).collect();
    let (result, _) = delete_paths(running, &items);
    let dropped = drop_infos(leaving);
    let shown = leaving
        .iter()
        .map(|left| left.trash.root.clone())
        .filter(|root| dropped.iter().any(|changed| changed.starts_with(root)))
        .collect();
    ended(result, shown)
}

fn ended(result: Result<(), Stop>, shown: BTreeSet<PathBuf>) -> Ended {
    (
        result.map_or_else(Stop::outcome, |()| Outcome::Done),
        shown.into_iter().collect(),
    )
}

/// Each of `items` as an entry leaving its trash, once each; on a
/// resume, an entry that took a name the job freed is left out.
///
/// # Errors
/// [`Stop`] when one can't be looked at, or isn't an entry a trash
/// lists, so nothing is changed.
fn leaving(running: &Running, items: &[PathBuf], trashes: &Trashes) -> Result<Vec<Leaving>, Stop> {
    let mut seen = HashSet::new();
    let mut leaving = Vec::new();
    for item in items.iter().filter(|item| seen.insert(*item)) {
        item.symlink_metadata()
            .map_err(|error| Stop::at(item, &error))?;
        let (trash, listed) =
            trash::find(item, trashes).ok_or_else(|| Stop::because(item, Failure::Unsupported))?;
        if let Some(left) = leave(running, trash, listed).map_err(|error| Stop::at(item, &error))? {
            leaving.push(left);
        }
    }
    Ok(leaving)
}

/// `listed`, an entry of `trash`, as leaving it; `None` when a resumed
/// job finds it isn't the entry it was about to take.
///
/// # Errors
/// When it or its info file can't be looked at.
fn leave(running: &Running, trash: TrashFolder, listed: Trashed) -> io::Result<Option<Leaving>> {
    let item_entry = Entry::of(&listed.item.symlink_metadata()?);
    if running
        .before
        .leaving
        .get(&listed.item)
        .is_some_and(|before| *before != item_entry)
    {
        return Ok(None);
    }
    let name = listed.item.file_name().unwrap_or_default();
    let info = trash::info_of(&trash, name);
    let info_entry = Entry::of(&info.symlink_metadata()?);
    Ok(Some(Leaving {
        item: listed.item,
        info,
        info_entry,
        item_entry,
        original: listed.original,
        trash,
    }))
}

/// Makes the folders each restored entry was in, when they are gone and
/// no other entry of `leaving` is one of them, and notes in `touched` the
/// folder each first one was made in.
///
/// # Errors
/// [`Stop`] when cancelled, or when one can't be made.
fn make_folders(
    running: &Running,
    leaving: &[Leaving],
    touched: &mut BTreeSet<PathBuf>,
) -> Result<(), Stop> {
    for parent in leaving.iter().filter_map(|left| left.original.parent()) {
        let restored = leaving
            .iter()
            .any(|other| parent.starts_with(&other.original));
        if restored || parent.symlink_metadata().is_ok() {
            continue;
        }
        running.shared.control.checkpoint()?;
        let first = parent.ancestors().find(|above| {
            above
                .parent()
                .is_none_or(|up| up.symlink_metadata().is_ok())
        });
        fs::create_dir_all(parent).map_err(|error| Stop::at(parent, &error))?;
        touched.extend(first.and_then(Path::parent).map(Path::to_path_buf));
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
