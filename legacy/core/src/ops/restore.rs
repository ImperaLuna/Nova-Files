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

struct Leaving {
    item: PathBuf,
    info: PathBuf,
    info_entry: Entry,
    item_entry: Entry,
    original: PathBuf,
    trash: TrashFolder,
}

pub fn restore(running: &mut Running, items: &[PathBuf], trashes: &Trashes) -> Ended {
    running.record_job(word::RESTORE, None, items);
    put_back(running, items, trashes)
}

pub fn put_back(running: &mut Running, items: &[PathBuf], trashes: &Trashes) -> Ended {
    let mut leaving = match leaving(running, items, trashes) {
        Ok(leaving) => leaving,
        Err(stop) => return (stop.outcome(), Vec::new()),
    };
    leaving.sort_by_key(|left| left.original.components().count());
    let mut touched = BTreeSet::new();
    let originals: Vec<_> = leaving.iter().map(|left| left.original.as_path()).collect();
    if let Err(stop) = make_folders(running, &originals, &mut touched) {
        return (stop.outcome(), touched.into_iter().collect());
    }
    note_infos(running, &leaving);
    let pairs: Vec<_> = leaving
        .iter()
        .map(|left| (left.item.clone(), left.original.clone()))
        .collect();
    let (result, moved) = transfer_to(running, &pairs, How::Move);
    let dropped = drop_infos(&leaving);
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

pub fn delete_from_trash(running: &mut Running, items: &[PathBuf], trashes: &Trashes) -> Ended {
    running.record_job(word::DELETE_FROM_TRASH, None, items);
    match leaving(running, items, trashes) {
        Ok(leaving) => delete_leaving(running, &leaving),
        Err(stop) => (stop.outcome(), Vec::new()),
    }
}

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

fn leave(running: &Running, trash: TrashFolder, listed: Trashed) -> io::Result<Option<Leaving>> {
    let item_entry = Entry::of(&listed.item.symlink_metadata()?);
    if running
        .before
        .leaving
        .get(&listed.item)
        .is_some_and(|before| !before.is(&item_entry))
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

pub fn make_folders(
    running: &Running,
    targets: &[&Path],
    touched: &mut BTreeSet<PathBuf>,
) -> Result<(), Stop> {
    for parent in targets.iter().filter_map(|target| target.parent()) {
        let restored = targets.iter().any(|other| parent.starts_with(other));
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
