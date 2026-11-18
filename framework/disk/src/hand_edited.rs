use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError, Weak};

use crate::private_file::{self, Read};

pub trait Reread {
    type Read;

    fn path(&self) -> Option<PathBuf>;

    fn version(&self) -> u64;

    fn take(&mut self, read: Self::Read, version: u64) -> bool;
}

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

const MAX_READS: usize = 8;

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
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::Arc;

    use super::*;

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
