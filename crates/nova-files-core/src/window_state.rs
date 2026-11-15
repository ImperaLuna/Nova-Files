//! The UI's window state, kept for it between runs.
//!
//! The UI owns what the state says (window sizes, for now); the core only
//! keeps the bytes, in memory while it runs and in
//! `$XDG_STATE_HOME/nova-files/window-state` between runs. The file is read
//! once, when the core starts; a missing, unreadable or oversized one reads
//! as empty. Saves go through a [`Saver`], in order and off the core's
//! thread; a save that fails is logged and the state stays in memory for
//! this run.

use std::cell::RefCell;
use std::path::Path;
use std::sync::Arc;

use crate::private_file::{self, Read};
use crate::saver::Saver;

/// The file, under the state folder.
const FILE: &str = "nova-files/window-state";
/// A state this big is not one the UI wrote; it is never read or kept.
pub const MAX_BYTES: usize = 64 * 1024;
const SAVE_THREAD: &str = "nova-files-state";
/// The file, as the log names it.
const WHAT: &str = "window state";

/// The window state, as the core keeps it.
pub struct Store {
    bytes: RefCell<Arc<[u8]>>,
    /// Saves it; to nowhere without a home folder, which keeps it in
    /// memory only.
    saver: Saver,
}

impl Store {
    /// The state saved under the state folder `state_home`.
    pub fn open(state_home: Option<&Path>) -> Self {
        let path = state_home.map(|home| home.join(FILE));
        let bytes = path.as_deref().map(read).unwrap_or_default();
        Self {
            bytes: RefCell::new(bytes.into()),
            saver: Saver::new(path, WHAT, SAVE_THREAD),
        }
    }

    /// The state as last saved.
    pub fn bytes(&self) -> Vec<u8> {
        self.bytes.borrow().to_vec()
    }

    /// Keeps `bytes` as the state and writes it out. An oversized state is
    /// dropped.
    pub fn save(&self, bytes: Vec<u8>) {
        if bytes.len() > MAX_BYTES {
            return;
        }
        let bytes: Arc<[u8]> = bytes.into();
        *self.bytes.borrow_mut() = Arc::clone(&bytes);
        self.saver.save(bytes);
    }
}

/// The file's bytes, or none when it is missing, unreadable or oversized.
fn read(path: &Path) -> Vec<u8> {
    match private_file::read(path, MAX_BYTES) {
        Read::Bytes(bytes) => bytes,
        Read::Missing | Read::TooBig | Read::Failed(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::thread;

    use super::*;

    fn temp(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("nova-files-state-{name}-{}", std::process::id()));
        fs::remove_dir_all(&path).ok();
        path
    }

    #[test]
    fn a_missing_or_oversized_file_reads_as_empty() {
        let home = temp("read");
        let path = home.join(FILE);
        assert_eq!(read(&path), b"");
        private_file::replace(&path, &vec![b'x'; MAX_BYTES], private_file::FILE_MODE).unwrap();
        assert_eq!(read(&path).len(), MAX_BYTES);
        private_file::replace(&path, &vec![b'x'; MAX_BYTES + 1], private_file::FILE_MODE).unwrap();
        assert_eq!(read(&path), b"");
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn the_store_keeps_what_was_saved_and_reads_it_back_next_time() {
        let home = temp("store");
        let store = Store::open(Some(&home));
        assert_eq!(store.bytes(), b"");
        store.save(b"picker 900x600".to_vec());
        assert_eq!(store.bytes(), b"picker 900x600");
        // An oversized state is not kept; one at the cap, 64 KiB, is.
        store.save(vec![b'x'; MAX_BYTES + 1]);
        assert_eq!(store.bytes(), b"picker 900x600");
        store.save(vec![b'x'; 64 * 1024]);
        assert_eq!(store.bytes().len(), 64 * 1024);
        // The last save differs from every earlier one, so once the file
        // holds it no earlier write can still land.
        store.save(b"picker 800x500".to_vec());
        let path = home.join(FILE);
        let wait = std::time::Instant::now();
        while read(&path) != b"picker 800x500" {
            assert!(
                wait.elapsed() < std::time::Duration::from_secs(10),
                "never saved"
            );
            thread::yield_now();
        }
        assert_eq!(Store::open(Some(&home)).bytes(), b"picker 800x500");
        let memory_only = Store::open(None);
        memory_only.save(b"main 1x1".to_vec());
        assert_eq!(memory_only.bytes(), b"main 1x1");
        fs::remove_dir_all(home).ok();
    }
}
