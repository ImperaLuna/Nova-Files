//! Reading one folder: its entries, folders first, each group A to Z.
//!
//! [`read_folder`] asks `cancelled` before each read from the disk, so a
//! listing nobody wants any more stops early. Links are followed only to
//! learn whether they point at a folder.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::Path;

use nova_files_api::{Entry, EntryId, EntryKind, Unreadable};

/// Why [`read_folder`] returned without entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    Unreadable(Unreadable),
    /// `cancelled` said so; nobody wants the answer.
    Cancelled,
}

impl From<io::Error> for Stopped {
    fn from(error: io::Error) -> Self {
        Self::Unreadable(unreadable(&error))
    }
}

/// Lists the folder at `path`, sorted.
///
/// # Errors
/// [`Stopped`] when the folder can't be read or the listing was cancelled.
/// An entry that vanishes while the folder is read is left out.
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
    // Asked before each step, since each step can read the disk.
    while !cancelled() {
        let Some(item) = items.next() else {
            found.sort_by_cached_key(sort_key);
            return Ok(numbered(found));
        };
        match item.and_then(|item| Ok((item.file_name(), kind_of(&item)?))) {
            Ok(entry) => found.push(entry),
            Err(error) => vanished(error)?,
        }
    }
    Err(Stopped::Cancelled)
}

/// Passes over an entry that vanished between the folder read and this
/// one: it is simply not there any more. Any other error is the folder's.
fn vanished(error: io::Error) -> Result<(), Stopped> {
    if error.kind() == io::ErrorKind::NotFound {
        Ok(())
    } else {
        Err(error.into())
    }
}

/// The sorted entries, each with its position as its id.
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
        // A broken link is not an error: it is a file you can't open.
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

/// Folders before files; then by name ignoring case; then by the exact
/// bytes, so the order is the same on every run. Worked out once per entry,
/// not once per comparison.
fn sort_key((name, kind): &(OsString, EntryKind)) -> (bool, String, OsString) {
    (
        *kind == EntryKind::File,
        name.to_string_lossy().to_lowercase(),
        name.clone(),
    )
}

/// Sorts one `io::Error` into the reasons a client acts on.
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
}
