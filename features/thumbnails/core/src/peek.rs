use std::path::Path;

use nova_files_api::{Entry, EntryKind, FileTypes};
use nova_files_entries_core::list;
use nova_files_thumbnails_api::PEEKED;

const MOVING_PICTURES: [&str; 2] = ["image/*", "video/*"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    Picture,
    File,
    Folder,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Pick {
    pub entry: Entry,
    pub picture: bool,
}

pub fn look(folder: &Path, types: &dyn FileTypes, stop: impl Fn() -> bool) -> Option<Vec<Pick>> {
    let entries = list::read_folder(folder, stop).ok()?;
    Some(pick(entries, types))
}

pub fn pick(entries: Vec<Entry>, types: &dyn FileTypes) -> Vec<Pick> {
    let mut ranked: Vec<(bool, Rank, Entry)> = entries
        .into_iter()
        .map(|entry| (entry.is_hidden(), rank(&entry, types), entry))
        .collect();
    ranked.sort_by_key(|(hidden, rank, _)| (*hidden, *rank));
    let any_shown = ranked.first().is_some_and(|(hidden, _, _)| !hidden);
    ranked
        .into_iter()
        .take_while(|(hidden, _, _)| !(any_shown && *hidden))
        .take(PEEKED)
        .map(|(_, rank, entry)| Pick {
            entry,
            picture: rank == Rank::Picture,
        })
        .collect()
}

fn rank(entry: &Entry, types: &dyn FileTypes) -> Rank {
    if entry.kind == EntryKind::Folder {
        return Rank::Folder;
    }
    let moving_picture = types.of(&entry.name).is_some_and(|mime| {
        MOVING_PICTURES
            .iter()
            .any(|wanted| types.is_a(mime, wanted))
    });
    if moving_picture {
        Rank::Picture
    } else {
        Rank::File
    }
}
