//! The UI's window state, kept for it between runs.
//!
//! The UI owns what the state says (window sizes, for now); the core only
//! keeps the bytes, in memory while it runs and in
//! `$XDG_STATE_HOME/nova-files/window-state` between runs. The file is read
//! once, when the core starts; a missing, unreadable or oversized one reads
//! as empty. A save replaces the file whole, through a temporary file and a
//! rename, on a thread of its own, so a crash never leaves half a file and
//! a slow disk never holds up the core.

use std::cell::RefCell;
use std::fs;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

/// The file, under the state folder.
const FILE: &str = "nova-files/window-state";
/// A state this big is not one the UI wrote; it is never read or kept.
pub const MAX_BYTES: usize = 64 * 1024;
const SAVE_THREAD: &str = "nova-files-state";

/// Tells apart the temporary files of saves that overlap.
static SAVES: AtomicU64 = AtomicU64::new(0);

/// The window state, as the core keeps it.
pub struct Store {
    /// Where it is saved; `None` without a home folder, which keeps it in
    /// memory only.
    path: Option<PathBuf>,
    bytes: RefCell<Arc<[u8]>>,
}

impl Store {
    /// The state saved under the state folder `state_home`.
    pub fn open(state_home: Option<&Path>) -> Self {
        let path = state_home.map(|home| home.join(FILE));
        let bytes = path.as_deref().map(read).unwrap_or_default();
        Self {
            path,
            bytes: RefCell::new(bytes.into()),
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
        let Some(path) = self.path.clone() else {
            return;
        };
        let wrote = thread::Builder::new()
            .name(SAVE_THREAD.to_owned())
            .spawn(move || write(&path, &bytes));
        // Without a thread the state stays in memory for this run.
        drop(wrote);
    }
}

/// The file's bytes, or none when it is missing, unreadable or oversized.
fn read(path: &Path) -> Vec<u8> {
    let mut bytes = Vec::new();
    let read = fs::File::open(path).and_then(|file| {
        file.take(u64::try_from(MAX_BYTES).unwrap_or(u64::MAX) + 1)
            .read_to_end(&mut bytes)
    });
    if read.is_err() || bytes.len() > MAX_BYTES {
        return Vec::new();
    }
    bytes
}

/// Replaces the file at `path` with `bytes` whole, making its folder.
fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    let save = SAVES.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_extension(format!("tmp-{}-{save}", std::process::id()));
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path).inspect_err(|_| {
        fs::remove_file(&temporary).ok();
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("nova-files-state-{name}-{}", std::process::id()));
        fs::remove_dir_all(&path).ok();
        path
    }

    #[test]
    fn a_write_replaces_the_file_whole_and_leaves_no_temporary() {
        let home = temp("write");
        let path = home.join(FILE);
        write(&path, b"picker 900x600").unwrap();
        write(&path, b"picker 1000x700").unwrap();
        assert_eq!(read(&path), b"picker 1000x700");
        let left: Vec<_> = fs::read_dir(path.parent().unwrap()).unwrap().collect();
        assert_eq!(left.len(), 1);
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn a_missing_or_oversized_file_reads_as_empty() {
        let home = temp("read");
        let path = home.join(FILE);
        assert_eq!(read(&path), b"");
        write(&path, &vec![b'x'; MAX_BYTES]).unwrap();
        assert_eq!(read(&path).len(), MAX_BYTES);
        write(&path, &vec![b'x'; MAX_BYTES + 1]).unwrap();
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
        store.save(b"picker 900x600".to_vec());
        let path = home.join(FILE);
        let wait = std::time::Instant::now();
        while read(&path) != b"picker 900x600" {
            assert!(
                wait.elapsed() < std::time::Duration::from_secs(10),
                "never saved"
            );
            thread::yield_now();
        }
        assert_eq!(Store::open(Some(&home)).bytes(), b"picker 900x600");
        let memory_only = Store::open(None);
        memory_only.save(b"main 1x1".to_vec());
        assert_eq!(memory_only.bytes(), b"main 1x1");
        fs::remove_dir_all(home).ok();
    }
}
