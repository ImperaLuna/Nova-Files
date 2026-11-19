use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::Path;

use nova_files_api::{Entry, EntryId, EntryKind, HiddenFiles, Unreadable};

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
