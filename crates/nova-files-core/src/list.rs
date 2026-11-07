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
    let mut found = Vec::new();
    for item in fs::read_dir(path)? {
        if cancelled() {
            return Err(Stopped::Cancelled);
        }
        // An entry that vanished between the folder read and this one is
        // simply not there any more.
        let Ok(item) = item else { continue };
        let Ok(kind) = kind_of(&item) else { continue };
        found.push((item.file_name(), kind));
    }
    found.sort_by_cached_key(sort_key);
    let entries = found
        .into_iter()
        .zip(0..)
        .map(|((name, kind), index)| Entry {
            id: EntryId(index),
            name,
            kind,
        })
        .collect();
    Ok(entries)
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
