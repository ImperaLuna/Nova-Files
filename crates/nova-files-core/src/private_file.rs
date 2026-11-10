//! Reading and saving the small files the core keeps for the user: the
//! window state, the settings and the pinned folders.
//!
//! [`read`] reads a file up to a cap, so a huge file is never loaded.
//! [`replace`] saves one whole: it writes a temporary file beside it,
//! with its mode from the start, and renames it over the old one, so a crash
//! never leaves half a file and a reader sees the old file or the new one.

use std::fs::{self, DirBuilder, OpenOptions, Permissions};
use std::io::{self, Read as _, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The files and the folders made for them are the user's alone.
pub const FILE_MODE: u32 = 0o600;
pub const FOLDER_MODE: u32 = 0o700;

/// Tells apart the temporary files of saves that overlap.
static SAVES: AtomicU64 = AtomicU64::new(0);

/// What [`read`] found.
#[derive(Debug)]
pub enum Read {
    Bytes(Vec<u8>),
    /// There is no file.
    Missing,
    /// The file is over the cap; it was not read.
    TooBig,
    /// The file is there but couldn't be read.
    Failed(io::Error),
}

/// The bytes of the file at `path`, when there are at most `max_bytes`.
pub fn read(path: &Path, max_bytes: usize) -> Read {
    let mut bytes = Vec::new();
    let cap = u64::try_from(max_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    match fs::File::open(path).and_then(|file| file.take(cap).read_to_end(&mut bytes)) {
        Ok(_) if bytes.len() > max_bytes => Read::TooBig,
        Ok(_) => Read::Bytes(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Read::Missing,
        Err(error) => Read::Failed(error),
    }
}

/// Replaces the file at `path` with `bytes` whole, making its folder
/// private; the new file has `mode` from the start.
pub fn replace(path: &Path, bytes: &[u8], mode: u32) -> io::Result<()> {
    if let Some(folder) = path.parent() {
        DirBuilder::new()
            .recursive(true)
            .mode(FOLDER_MODE)
            .create(folder)?;
    }
    let save = SAVES.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_extension(format!("tmp-{}-{save}", std::process::id()));
    let written = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(&temporary)
        // The umask took bits off `mode`; they are put back.
        .and_then(|file| {
            file.set_permissions(Permissions::from_mode(mode))
                .map(|()| file)
        })
        .and_then(|mut file| file.write_all(bytes))
        .and_then(|()| fs::rename(&temporary, path));
    if written.is_err() {
        fs::remove_file(&temporary).ok();
    }
    written
}

/// The permission bits of a file's mode, kept across a save.
const PERMISSION_BITS: u32 = 0o7777;

/// The mode a save of `path` gives the new file: the old file's own, so a
/// file the user made keeps it; [`FILE_MODE`] for a new one.
pub fn mode_to_keep(path: &Path) -> u32 {
    fs::metadata(path).map_or(FILE_MODE, |metadata| {
        metadata.permissions().mode() & PERMISSION_BITS
    })
}

/// Where a save of `path` goes: the file a symlink points to, so a linked
/// file stays linked; `path` itself otherwise.
pub fn saved_at(path: &Path) -> PathBuf {
    let is_link = fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_symlink());
    if is_link {
        // A link to nowhere is saved over: there is nothing to keep.
        fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
    } else {
        path.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The permission bits of a mode.
    const ALL_MODE_BITS: u32 = 0o777;
    const CAP: usize = 16;

    fn temp(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("nova-files-private-{name}-{}", std::process::id()));
        fs::remove_dir_all(&path).ok();
        path
    }

    fn bytes(read: Read) -> Vec<u8> {
        match read {
            Read::Bytes(bytes) => bytes,
            other => panic!("expected bytes, got {other:?}"),
        }
    }

    #[test]
    fn a_replace_swaps_the_file_whole_and_leaves_no_temporary() {
        let home = temp("replace");
        let path = home.join("folder/file");
        replace(&path, b"picker 900x600", FILE_MODE).unwrap();
        replace(&path, b"picker 1000x700", FILE_MODE).unwrap();
        assert_eq!(bytes(read(&path, CAP)), b"picker 1000x700");
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & ALL_MODE_BITS;
        assert_eq!(mode(&path), FILE_MODE);
        assert_eq!(mode(path.parent().unwrap()), FOLDER_MODE);
        let left: Vec<_> = fs::read_dir(path.parent().unwrap()).unwrap().collect();
        assert_eq!(left.len(), 1);
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn a_replace_keeps_bits_the_umask_would_take() {
        let home = temp("umask");
        let path = home.join("folder/file");
        // Group write: the usual umask takes it off a new file.
        let shared = 0o664;
        replace(&path, b"shared", shared).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & ALL_MODE_BITS;
        assert_eq!(mode, shared);
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn a_read_tells_missing_oversized_and_unreadable_apart() {
        let home = temp("read");
        let path = home.join("file");
        assert!(matches!(read(&path, CAP), Read::Missing));
        replace(&path, &[b'x'; CAP], FILE_MODE).unwrap();
        assert_eq!(bytes(read(&path, CAP)).len(), CAP);
        replace(&path, &[b'x'; CAP + 1], FILE_MODE).unwrap();
        assert!(matches!(read(&path, CAP), Read::TooBig));
        // A folder where the file should be opens but can't be read.
        assert!(matches!(read(&home, CAP), Read::Failed(_)));
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn a_failed_replace_is_an_error() {
        let home = temp("blocked");
        // A file where the folder should be.
        fs::write(&home, b"").unwrap();
        assert!(replace(&home.join("file"), b"x", FILE_MODE).is_err());
        fs::remove_file(home).ok();
    }
}
