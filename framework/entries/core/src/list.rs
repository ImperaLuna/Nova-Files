use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::Path;

use nova_files_api::{Entry, EntryId, EntryKind, HiddenFiles, Unreadable, holds_every_word};

use crate::folder::Inner;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    Unreadable(Unreadable),
    Cancelled,
}

impl From<io::Error> for Stopped {
    fn from(error: io::Error) -> Self {
        Self::Unreadable(unreadable(&error))
    }
}

pub fn read_folder(path: &Path, cancelled: impl Fn() -> bool) -> Result<Vec<Entry>, Stopped> {
    if cancelled() {
        return Err(Stopped::Cancelled);
    }
    if !fs::metadata(path)?.is_dir() {
        return Err(Stopped::Unreadable(Unreadable::NotAFolder));
    }
    if cancelled() {
        return Err(Stopped::Cancelled);
    }
    let mut items = fs::read_dir(path)?;
    let mut found = Vec::new();
    while !cancelled() {
        let Some(item) = items.next() else {
            found.sort_by_cached_key(sort_key);
            return Ok(numbered(found));
        };
        match item {
            Ok(item) => {
                found.extend(kind_or_gone(kind_of(&item)).map(|kind| (item.file_name(), kind)));
            }
            Err(error) => vanished(error)?,
        }
    }
    Err(Stopped::Cancelled)
}

pub fn keep_shown(entries: Vec<Entry>, hidden: HiddenFiles) -> Vec<Entry> {
    match hidden {
        HiddenFiles::Shown => entries,
        HiddenFiles::Left => numbered(
            entries
                .into_iter()
                .filter(|entry| !entry.is_hidden())
                .map(|entry| (entry.name, entry.kind))
                .collect(),
        ),
    }
}

pub fn keep_named(entries: Vec<Entry>, filter: &str) -> Vec<Entry> {
    let paired = entries.into_iter().map(|entry| (entry, ())).collect();
    keep_named_paired(paired, filter)
        .into_iter()
        .map(|(entry, ())| entry)
        .collect()
}

pub fn keep_named_known(listed: Inner, filter: &str) -> Inner {
    let (entries, details) = listed?;
    let paired = entries.into_iter().zip(details).collect();
    Ok(keep_named_paired(paired, filter).into_iter().unzip())
}

fn keep_named_paired<T>(paired: Vec<(Entry, T)>, filter: &str) -> Vec<(Entry, T)> {
    if filter.trim().is_empty() {
        return paired;
    }
    paired
        .into_iter()
        .filter(|(entry, _)| holds_every_word(&entry.name.to_string_lossy(), filter))
        .zip(0..)
        .map(|((entry, carried), index)| {
            let entry = Entry {
                id: EntryId(index),
                ..entry
            };
            (entry, carried)
        })
        .collect()
}

fn vanished(error: io::Error) -> Result<(), Stopped> {
    if error.kind() == io::ErrorKind::NotFound {
        Ok(())
    } else {
        Err(error.into())
    }
}

fn kind_or_gone(kind: io::Result<EntryKind>) -> Option<EntryKind> {
    match kind {
        Ok(kind) => Some(kind),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(_) => Some(EntryKind::File),
    }
}

fn numbered(found: Vec<(OsString, EntryKind)>) -> Vec<Entry> {
    found
        .into_iter()
        .zip(0..)
        .map(|((name, kind), index)| Entry {
            id: EntryId(index),
            name,
            kind,
        })
        .collect()
}

fn kind_of(item: &fs::DirEntry) -> io::Result<EntryKind> {
    let file_type = item.file_type()?;
    let is_folder = if file_type.is_symlink() {
        fs::metadata(item.path()).is_ok_and(|target| target.is_dir())
    } else {
        file_type.is_dir()
    };
    Ok(if is_folder {
        EntryKind::Folder
    } else {
        EntryKind::File
    })
}

pub fn sort_key((name, kind): &(OsString, EntryKind)) -> (bool, String, OsString) {
    (
        *kind == EntryKind::File,
        name.to_string_lossy().to_lowercase(),
        name.clone(),
    )
}

pub fn unreadable(error: &io::Error) -> Unreadable {
    match error.kind() {
        io::ErrorKind::NotFound => Unreadable::Missing,
        io::ErrorKind::PermissionDenied => Unreadable::PermissionDenied,
        io::ErrorKind::NotADirectory => Unreadable::NotAFolder,
        _ => Unreadable::Failed,
    }
}

#[cfg(test)]
mod tests {
    use nova_files_api::Details;

    use super::*;

    fn sorted(items: &[(&str, EntryKind)]) -> Vec<String> {
        let mut items: Vec<(OsString, EntryKind)> = items
            .iter()
            .map(|(name, kind)| (OsString::from(name), *kind))
            .collect();
        items.sort_by_cached_key(sort_key);
        items
            .into_iter()
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect()
    }

    fn files(names: &[&str]) -> Vec<Entry> {
        numbered(
            names
                .iter()
                .map(|name| (OsString::from(name), EntryKind::File))
                .collect(),
        )
    }

    #[test]
    fn a_filter_keeps_the_names_holding_its_words_and_renumbers_them() {
        let kept = keep_named(files(&["Beach.jpg", "notes.txt", "beach.png"]), "BEACH");
        assert_eq!(kept, files(&["Beach.jpg", "beach.png"]));
        let all = files(&["a", "b"]);
        assert_eq!(keep_named(all.clone(), " "), all);
    }

    #[test]
    fn a_filter_keeps_each_entry_with_its_own_details() {
        let details = |size| Details {
            size: Some(size),
            modified: None,
        };
        let listed = Ok((
            files(&["one", "two", "three"]),
            vec![details(1), details(2), details(3)],
        ));
        let (entries, kept) = keep_named_known(listed, "t").unwrap();
        assert_eq!(entries, files(&["two", "three"]));
        assert_eq!(kept, [details(2), details(3)]);
        assert_eq!(
            keep_named_known(Err(Unreadable::Failed), "t"),
            Err(Unreadable::Failed)
        );
    }

    #[test]
    fn only_a_vanished_entry_is_passed_over() {
        assert_eq!(vanished(io::ErrorKind::NotFound.into()), Ok(()));
        assert_eq!(
            vanished(io::Error::other("disk failed")),
            Err(Stopped::Unreadable(Unreadable::Failed))
        );
    }

    #[test]
    fn an_entry_whose_kind_cant_be_read_stays_listed_as_a_file() {
        assert_eq!(kind_or_gone(Ok(EntryKind::Folder)), Some(EntryKind::Folder));
        assert_eq!(kind_or_gone(Err(io::ErrorKind::NotFound.into())), None);
        assert_eq!(
            kind_or_gone(Err(io::Error::other("disk failed"))),
            Some(EntryKind::File)
        );
    }

    #[test]
    fn folders_come_first_then_names_ignore_case() {
        let order = sorted(&[
            ("b.txt", EntryKind::File),
            ("Zeta", EntryKind::Folder),
            ("A.txt", EntryKind::File),
            ("alpha", EntryKind::Folder),
        ]);
        assert_eq!(order, ["alpha", "Zeta", "A.txt", "b.txt"]);
    }

    #[test]
    fn names_equal_but_for_case_keep_a_fixed_order() {
        let order = sorted(&[("readme", EntryKind::File), ("README", EntryKind::File)]);
        assert_eq!(order, ["README", "readme"]);
    }

    #[test]
    fn errors_map_to_reasons() {
        let reason = |kind| unreadable(&io::Error::from(kind));
        assert_eq!(reason(io::ErrorKind::NotFound), Unreadable::Missing);
        assert_eq!(
            reason(io::ErrorKind::PermissionDenied),
            Unreadable::PermissionDenied
        );
        assert_eq!(reason(io::ErrorKind::NotADirectory), Unreadable::NotAFolder);
        assert_eq!(reason(io::ErrorKind::Other), Unreadable::Failed);
    }

    #[test]
    fn hidden_entries_are_left_out_unless_shown_and_the_rest_renumbered() {
        let entries = numbered(vec![
            (OsString::from(".config"), EntryKind::Folder),
            (OsString::from("Projects"), EntryKind::Folder),
            (OsString::from(".bashrc"), EntryKind::File),
            (OsString::from("notes.txt"), EntryKind::File),
        ]);
        assert_eq!(keep_shown(entries.clone(), HiddenFiles::Shown), entries);
        let kept = keep_shown(entries, HiddenFiles::Left);
        assert_eq!(
            kept,
            numbered(vec![
                (OsString::from("Projects"), EntryKind::Folder),
                (OsString::from("notes.txt"), EntryKind::File),
            ])
        );
        assert_eq!(kept[1].id, EntryId(1));
    }
}
