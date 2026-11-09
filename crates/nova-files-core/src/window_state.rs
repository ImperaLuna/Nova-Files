//! The UI's window state, kept for it between runs.
//!
//! The UI owns what the state says (window sizes, for now); the core only
//! keeps the bytes, in memory while it runs and in
//! `$XDG_STATE_HOME/nova-files/window-state` between runs. The file is read
//! once, when the core starts; a missing, unreadable or oversized one reads
//! as empty. A save replaces the file whole, through a temporary file and a
//! rename, so a crash never leaves half a file. Saves go to one writer
//! thread in the order they came, so the file always ends with the last
//! one, and a slow disk never holds up the core. A save that fails is
//! logged and the state stays in memory for this run.

use std::cell::RefCell;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Read as _, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

/// The file, under the state folder.
const FILE: &str = "nova-files/window-state";
/// A state this big is not one the UI wrote; it is never read or kept.
pub const MAX_BYTES: usize = 64 * 1024;
const SAVE_THREAD: &str = "nova-files-state";
/// The file and the folders made for it are the user's alone.
const FILE_MODE: u32 = 0o600;
const FOLDER_MODE: u32 = 0o700;

/// Tells apart the temporary files of saves that overlap.
static SAVES: AtomicU64 = AtomicU64::new(0);

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
        if let Err(error) = write(path, &bytes) {
            log::warn!("can't save the window state to {}: {error}", path.display());
        }
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

/// Replaces the file at `path` with `bytes` whole, making its folder;
/// both are private from the start.
fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
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
        .mode(FILE_MODE)
        .open(&temporary)
        .and_then(|mut file| file.write_all(bytes))
        .and_then(|()| fs::rename(&temporary, path));
    if written.is_err() {
        fs::remove_file(&temporary).ok();
    }
    written
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    /// The permission bits of a mode.
    const ALL_MODE_BITS: u32 = 0o777;

    /// Saves queued before the writer runs.
    const SAVES_IN_A_ROW: u32 = 20;

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
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & ALL_MODE_BITS;
        assert_eq!(mode(&path), FILE_MODE);
        assert_eq!(mode(path.parent().unwrap()), FOLDER_MODE);
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

    #[test]
    fn a_failed_write_is_an_error() {
        let home = temp("blocked");
        // A file where the state folder should be.
        fs::write(&home, b"").unwrap();
        assert!(write(&home.join(FILE), b"picker 900x600").is_err());
        fs::remove_file(home).ok();
    }
}
