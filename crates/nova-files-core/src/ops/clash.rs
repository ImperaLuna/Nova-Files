//! Telling what stands in a copy's way: what already has the target's
//! name, and which failed steps are the user's to decide rather than the
//! job's end.

use std::io;
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;

use nova_files_api::{ConflictKind, Failure};

use super::plan::Kind;

/// What clashes with copying `source`, a `kind`, to `target`, or `None`
/// when the name is free. A source that is gone is a clash of its own;
/// only a folder's is looked for at a free name, as making a file or link
/// finds its source gone anyway, and one look per file adds up.
///
/// # Errors
/// When `target` can't be looked at.
pub fn clash(source: &Path, kind: Kind, target: &Path) -> io::Result<Option<ConflictKind>> {
    let there = match target.symlink_metadata() {
        Ok(there) => there,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let gone = kind == Kind::Folder && source.symlink_metadata().is_err();
            return Ok(gone.then_some(ConflictKind::SourceGone));
        }
        Err(error) => return Err(error),
    };
    let Ok(source) = source.symlink_metadata() else {
        return Ok(Some(ConflictKind::SourceGone));
    };
    let folder_over_folder = (kind == Kind::Folder, there.is_dir());
    Ok(Some(
        if source.dev() == there.dev() && source.ino() == there.ino() {
            ConflictKind::SameFile
        } else {
            match folder_over_folder {
                (true, true) => ConflictKind::FolderExists,
                (false, false) => ConflictKind::FileExists,
                _ => ConflictKind::KindMismatch,
            }
        },
    ))
}

/// The conflict a step that failed at `path` for `reason` raises, or
/// `None` when the failure ends the job. A taken name isn't here: the job
/// looks at the target again.
pub fn conflict_for(reason: Failure, path: &Path, source: &Path) -> Option<ConflictKind> {
    match reason {
        Failure::NoSpace => Some(ConflictKind::NoSpace),
        Failure::PermissionDenied => Some(ConflictKind::PermissionDenied),
        Failure::NotFound if path == source => Some(ConflictKind::SourceGone),
        Failure::SourceChanged => Some(ConflictKind::SourceChanged),
        Failure::Unsupported => Some(ConflictKind::Unsupported),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;

    use super::*;
    use crate::ops::test_folder::TestFolder;

    const FILE: Kind = Kind::File { size: 0 };

    #[test]
    fn names_what_has_the_target_name() {
        let folder = TestFolder::new("clash");
        let file = folder.file("file", "");
        let other = folder.file("other", "");
        let dir = folder.path().join("dir");
        fs::create_dir(&dir).unwrap();
        let link = folder.path().join("link");
        symlink(&dir, &link).unwrap();
        let free = folder.path().join("free");
        let gone = folder.path().join("gone");

        let clash = |source: &Path, kind, target: &Path| clash(source, kind, target).unwrap();
        assert_eq!(clash(&file, FILE, &free), None);
        assert_eq!(clash(&gone, FILE, &free), None, "making it finds it gone");
        assert_eq!(
            clash(&gone, Kind::Folder, &free),
            Some(ConflictKind::SourceGone)
        );
        assert_eq!(clash(&gone, FILE, &other), Some(ConflictKind::SourceGone));
        assert_eq!(clash(&file, FILE, &other), Some(ConflictKind::FileExists));
        assert_eq!(
            clash(&file, FILE, &link),
            Some(ConflictKind::FileExists),
            "a link isn't followed"
        );
        assert_eq!(
            clash(&dir, Kind::Folder, &dir),
            Some(ConflictKind::SameFile)
        );
        assert_eq!(clash(&file, FILE, &file), Some(ConflictKind::SameFile));
        let copy = folder.path().join("copy");
        fs::create_dir(&copy).unwrap();
        assert_eq!(
            clash(&dir, Kind::Folder, &copy),
            Some(ConflictKind::FolderExists)
        );
        assert_eq!(
            clash(&dir, Kind::Folder, &other),
            Some(ConflictKind::KindMismatch)
        );
        assert_eq!(clash(&file, FILE, &copy), Some(ConflictKind::KindMismatch));
    }

    #[test]
    fn some_failures_ask_and_others_end_the_job() {
        let (source, target) = (Path::new("/s"), Path::new("/t"));
        let ask = |reason, path| conflict_for(reason, path, source);
        assert_eq!(ask(Failure::NoSpace, target), Some(ConflictKind::NoSpace));
        assert_eq!(
            ask(Failure::PermissionDenied, source),
            Some(ConflictKind::PermissionDenied)
        );
        assert_eq!(
            ask(Failure::NotFound, source),
            Some(ConflictKind::SourceGone)
        );
        assert_eq!(
            ask(Failure::NotFound, target),
            None,
            "the target's folder went"
        );
        assert_eq!(
            ask(Failure::SourceChanged, source),
            Some(ConflictKind::SourceChanged)
        );
        assert_eq!(
            ask(Failure::Unsupported, source),
            Some(ConflictKind::Unsupported)
        );
        assert_eq!(ask(Failure::Other, source), None);
        assert_eq!(ask(Failure::NameTaken, target), None);
    }
}
