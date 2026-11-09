//! What the settings and theme stores share about the files people edit by
//! hand: reading one as text, taking it again when it changes, and their
//! locks.
//!
//! A store reads its file outside its lock, so a slow disk never holds up
//! the core, and carries a version from before the read: a read that finds
//! the store moved on since (a newer read taken, a change saved) is stale
//! and is done again rather than roll the store back.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError, Weak};

use crate::private_file::{self, Read};

/// A store whose file is read again when it changes.
pub trait Reread {
    /// What one read of the file gives.
    type Read;

    /// Where the file is; `None` without a config folder.
    fn path(&self) -> Option<PathBuf>;

    /// The store's version now, which a read carries.
    fn version(&self) -> u64;

    /// Takes `read`, done at `version`; says whether it was stale and must
    /// be done again.
    fn take(&mut self, read: Self::Read, version: u64) -> bool;
}

/// Reads the store's file with `read` until a read is taken, giving up
/// after [`MAX_READS`] stale ones: the store is changing faster than the
/// file can be read, and the change still to come reads it again.
pub fn reload<S: Reread>(weak: &Weak<Mutex<S>>, read: impl Fn(&Path) -> S::Read) {
    let Some(shared) = weak.upgrade() else {
        return;
    };
    for _ in 0..MAX_READS {
        let (path, version) = {
            let locked = lock(&shared);
            let Some(path) = locked.path() else {
                return;
            };
            (path, locked.version())
        };
        let file = read(&path);
        if !lock(&shared).take(file, version) {
            return;
        }
    }
    log::warn!("a hand-edited file kept changing while being read; read it next change");
}

/// How many reads in a row may come back stale before [`reload`] stops.
const MAX_READS: usize = 8;

/// The file at `path` as text, or `None` when it is missing or can't be
/// read (logged): the store then uses its defaults.
pub fn read_text(path: &Path, max_bytes: usize) -> Option<String> {
    match private_file::read(path, max_bytes) {
        Read::Bytes(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
        Read::Missing => None,
        Read::TooBig => {
            log::warn!(
                "{} is over {max_bytes} bytes, using defaults",
                path.display()
            );
            None
        }
        Read::Failed(error) => {
            log::warn!("can't read {}, using defaults: {error}", path.display());
            None
        }
    }
}

pub fn lock<T>(shared: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic while holding the lock left whole values behind: each field
    // is replaced in one step.
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::Arc;

    use super::*;

    /// A store that moves on before every read lands.
    struct Restless;

    impl Reread for Restless {
        type Read = ();

        fn path(&self) -> Option<PathBuf> {
            Some(PathBuf::from("restless.toml"))
        }

        fn version(&self) -> u64 {
            0
        }

        fn take(&mut self, (): (), _: u64) -> bool {
            true
        }
    }

    #[test]
    fn reads_that_keep_coming_back_stale_stop() {
        let shared = Arc::new(Mutex::new(Restless));
        let reads = Cell::new(0);
        reload(&Arc::downgrade(&shared), |_| reads.set(reads.get() + 1));
        assert_eq!(reads.get(), MAX_READS);
    }
}
