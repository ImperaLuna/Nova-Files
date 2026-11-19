use std::collections::{BTreeSet, HashSet};
use std::io;
use std::path::{Path, PathBuf};

use nova_files_api::{Failure, Outcome};
use nova_files_jobs::Ended;
use nova_files_jobs::failure::failure;
use nova_files_jobs::journal::{Record, word};
use nova_files_jobs::left::remove_lone_info;
use nova_files_jobs::mover::Mover;
use nova_files_jobs::parents::make_folders;
use nova_files_jobs::plan::Entry;
use nova_files_jobs::running::Running;
use nova_files_jobs::stop::Stop;
use nova_files_trash_api::Trashed;

use crate::trashes::{self as trash, TrashFolder, Trashes};

struct Leaving {
    item: PathBuf,
    info: PathBuf,
    info_entry: Entry,
    item_entry: Entry,
    original: PathBuf,
    trash: TrashFolder,
}

pub fn restore(
    running: &mut Running,
    items: &[PathBuf],
    trashes: &Trashes,
    files: &dyn Mover,
) -> Ended {
    running.record_job(word::RESTORE, None, items);
    put_back(running, items, trashes, files)
}

pub fn put_back(
    running: &mut Running,
    items: &[PathBuf],
    trashes: &Trashes,
    files: &dyn Mover,
) -> Ended {
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
    let (result, moved) = files.move_pairs(running, &pairs);
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

pub fn delete_from_trash(
    running: &mut Running,
    items: &[PathBuf],
    trashes: &Trashes,
    files: &dyn Mover,
) -> Ended {
    running.record_job(word::DELETE_FROM_TRASH, None, items);
    match leaving(running, items, trashes) {
        Ok(leaving) => delete_leaving(running, &leaving, files),
        Err(stop) => (stop.outcome(), Vec::new()),
    }
}

pub fn empty_trash(running: &mut Running, trashes: &Trashes, files: &dyn Mover) -> Ended {
    running.record_job(word::EMPTY_TRASH, None, &[]);
    let listed: Vec<_> = trashes
        .all()
        .into_iter()
        .flat_map(|trash| {
            trash::list(&trash)
                .into_iter()
                .map(move |listed| (trash.clone(), listed))
        })
        .collect();
    let mut leaving = Vec::new();
    for (trash, listed) in listed {
        let read = running.retry_or_skip(&listed.item, || match found(&trash, &listed) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            read => read.map(Some).map_err(|error| failure(&error)),
        });
        match read {
            Ok(Some(Some(read))) => leaving.extend(left(running, trash, listed, read)),
            Ok(_) => {}
            Err(stop) => return (stop.outcome(), Vec::new()),
        }
    }
    delete_leaving(running, &leaving, files)
}

fn delete_leaving(running: &mut Running, leaving: &[Leaving], files: &dyn Mover) -> Ended {
    note_infos(running, leaving);
    let items: Vec<_> = leaving.iter().map(|left| left.item.clone()).collect();
    let (result, _) = files.delete(running, &items);
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

struct Found {
    info: PathBuf,
    info_entry: Entry,
    item_entry: Entry,
}

fn found(trash: &TrashFolder, listed: &Trashed) -> io::Result<Found> {
    let item_entry = Entry::of(&listed.item.symlink_metadata()?);
    let name = listed.item.file_name().unwrap_or_default();
    let info = trash::info_of(trash, name);
    let info_entry = Entry::of(&info.symlink_metadata()?);
    Ok(Found {
        info,
        info_entry,
        item_entry,
    })
}

fn leave(running: &Running, trash: TrashFolder, listed: Trashed) -> io::Result<Option<Leaving>> {
    let found = found(&trash, &listed)?;
    Ok(left(running, trash, listed, found))
}

fn left(running: &Running, trash: TrashFolder, listed: Trashed, found: Found) -> Option<Leaving> {
    let changed = running
        .before
        .leaving
        .get(&listed.item)
        .is_some_and(|before| !before.is(&found.item_entry));
    (!changed).then_some(Leaving {
        item: listed.item,
        info: found.info,
        info_entry: found.info_entry,
        item_entry: found.item_entry,
        original: listed.original,
        trash,
    })
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
