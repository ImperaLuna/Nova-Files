//! The UI's window state, kept for it between runs.
//!
//! The UI owns what the state says (window sizes, for now); the core only
//! keeps the bytes, in memory while it runs and in
//! `$XDG_STATE_HOME/nova-files/window-state` between runs. The file is read
//! once, when the core starts; a missing, unreadable or oversized one reads
//! as empty. A save replaces the file whole (see `private_file`), so a
//! crash never leaves half a file. Saves go to one writer
//! thread in the order they came, so the file always ends with the last
//! one, and a slow disk never holds up the core. A save that fails is
//! logged and the state stays in memory for this run.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use crate::private_file::{self, Read};

/// The file, under the state folder.
const FILE: &str = "nova-files/window-state";
/// A state this big is not one the UI wrote; it is never read or kept.
pub const MAX_BYTES: usize = 64 * 1024;
const SAVE_THREAD: &str = "nova-files-state";

/// The window state, as the core keeps it.
pub struct Store {
    /// Where it is saved; `None` without a home folder, which keeps it in
    /// memory only.
    path: Option<PathBuf>,
    bytes: RefCell<Arc<[u8]>>,
    /// The writer thread's queue, started on the first save.
    writer: RefCell<Option<Sender<Arc<[u8]>>>>,
}

impl Store {
    /// The state saved under the state folder `state_home`.
    pub fn open(state_home: Option<&Path>) -> Self {
        let path = state_home.map(|home| home.join(FILE));
        let bytes = path.as_deref().map(read).unwrap_or_default();
        Self {
            path,
            bytes: RefCell::new(bytes.into()),
            writer: RefCell::default(),
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
        let Some(path) = &self.path else {
            return;
        };
        let mut writer = self.writer.borrow_mut();
        if writer.is_none() {
            *writer = start_writer(path.clone());
        }
        let sent = writer.as_ref().map(|queue| queue.send(bytes));
        if let Some(Err(_)) = sent {
            log::warn!("the window state writer stopped; the state isn't saved");
            *writer = None;
        }
    }
}

/// Starts the thread that writes each state it is sent to `path`, in order,
/// or none when no thread can start.
fn start_writer(path: PathBuf) -> Option<Sender<Arc<[u8]>>> {
    let (queue, saves) = mpsc::channel();
    let started = thread::Builder::new()
        .name(SAVE_THREAD.to_owned())
        .spawn(move || write_each(&path, &saves));
    match started {
        Ok(_) => Some(queue),
        Err(error) => {
            log::warn!("can't start the window state writer, the state isn't saved: {error}");
            None
        }
    }
}

/// Writes each state from `saves` to `path`, in order, until the store is
/// gone.
fn write_each(path: &Path, saves: &Receiver<Arc<[u8]>>) {
    for bytes in saves {
        if let Err(error) = private_file::replace(path, &bytes, private_file::FILE_MODE) {
            log::warn!("can't save the window state to {}: {error}", path.display());
        }
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

    use super::*;

    /// Saves queued before the writer runs.
    const SAVES_IN_A_ROW: u32 = 20;

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

    #[test]
    fn the_writer_writes_saves_in_order_and_goes_on_past_a_failure() {
        let home = temp("order");
        let path = home.join(FILE);
        let (queue, saves) = mpsc::channel();
        for save in 0..=SAVES_IN_A_ROW {
            queue.send(save.to_string().into_bytes().into()).unwrap();
        }
        drop(queue);
        write_each(&path, &saves);
        assert_eq!(read(&path), SAVES_IN_A_ROW.to_string().as_bytes());
        // A failed write is logged and the writer stops only with the store.
        let blocked = temp("order-blocked");
        fs::write(&blocked, b"").unwrap();
        let (queue, saves) = mpsc::channel();
        queue.send(b"x".to_vec().into()).unwrap();
        drop(queue);
        write_each(&blocked.join(FILE), &saves);
        fs::remove_file(blocked).ok();
        fs::remove_dir_all(home).ok();
    }
}
