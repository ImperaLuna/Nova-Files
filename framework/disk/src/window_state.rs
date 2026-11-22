use std::cell::RefCell;
use std::path::Path;
use std::sync::Arc;

use nova_files_api::WindowState;

use crate::private_file::{self, Read};
use crate::saver::Saver;

const FILE: &str = "nova-files/window-state";
pub const MAX_BYTES: usize = 64 * 1024;
const SAVE_THREAD: &str = "nova-files-state";
const WHAT: &str = "window state";

pub struct Store {
    bytes: RefCell<Arc<[u8]>>,
    saver: Saver,
}

impl Store {
    pub fn open(state_home: Option<&Path>) -> Self {
        let path = state_home.map(|home| home.join(FILE));
        let read = path.as_deref().map(read);
        let saved_to = path.filter(|_| read.as_ref().is_none_or(Option::is_some));
        Self {
            bytes: RefCell::new(read.flatten().unwrap_or_default().into()),
            saver: Saver::new(saved_to, WHAT, SAVE_THREAD),
        }
    }

    pub fn bytes(&self) -> Vec<u8> {
        self.bytes.borrow().to_vec()
    }

    pub fn save(&self, changed: &[u8]) {
        let changed = WindowState::parse(changed);
        if changed.is_empty() {
            return;
        }
        let bytes = WindowState::parse(&self.bytes()).merged(changed).to_bytes();
        if bytes.len() > MAX_BYTES {
            return;
        }
        let bytes: Arc<[u8]> = bytes.into();
        *self.bytes.borrow_mut() = Arc::clone(&bytes);
        self.saver.save(bytes);
    }
}

fn read(path: &Path) -> Option<Vec<u8>> {
    match private_file::read(path, MAX_BYTES) {
        Read::Bytes(bytes) => Some(bytes),
        Read::Missing => Some(Vec::new()),
        Read::TooBig => {
            log::warn!(
                "{} is over {MAX_BYTES} bytes; it isn't read or saved over",
                path.display()
            );
            None
        }
        Read::Failed(error) => {
            log::warn!(
                "can't read {}; it isn't saved over: {error}",
                path.display()
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::thread;

    use nova_files_test_folder::TestFolder;

    use super::*;

    fn temp(name: &str) -> (TestFolder, PathBuf) {
        let folder = TestFolder::new(&format!("state-{name}"));
        let path = folder.join(name);
        (folder, path)
    }

    #[test]
    fn a_missing_file_reads_as_empty_and_an_oversized_one_not_at_all() {
        let (_home_folder, home) = temp("read");
        let path = home.join(FILE);
        assert_eq!(read(&path), Some(Vec::new()));
        private_file::replace(&path, &vec![b'x'; MAX_BYTES], private_file::FILE_MODE).unwrap();
        assert_eq!(read(&path).map(|bytes| bytes.len()), Some(MAX_BYTES));
        private_file::replace(&path, &vec![b'x'; MAX_BYTES + 1], private_file::FILE_MODE).unwrap();
        assert_eq!(read(&path), None);
    }

    #[test]
    fn a_file_that_cant_be_read_is_never_saved_over() {
        let (_home_folder, home) = temp("unread");
        let file = home.join(FILE);
        fs::create_dir_all(&file).unwrap();
        let unreadable = Store::open(Some(&home));
        fs::remove_dir(&file).unwrap();
        private_file::replace(&file, &vec![b'x'; MAX_BYTES + 1], private_file::FILE_MODE).unwrap();
        let too_big = Store::open(Some(&home));
        fs::remove_file(&file).unwrap();
        let missing = Store::open(Some(&home));

        assert_eq!(unreadable.saver.path(), None);
        assert_eq!(too_big.saver.path(), None);
        assert_eq!(missing.saver.path(), Some(file.as_path()));
    }

    fn picker(size: &str) -> Vec<u8> {
        let mut state = WindowState::default();
        state.set("picker", size);
        state.to_bytes()
    }

    #[test]
    fn the_store_keeps_what_was_saved_and_reads_it_back_next_time() {
        let (_home_folder, home) = temp("store");
        let store = Store::open(Some(&home));
        assert_eq!(store.bytes(), b"");
        store.save(&picker("900x600"));
        assert_eq!(store.bytes(), picker("900x600"));
        store.save(&picker(&"9".repeat(MAX_BYTES)));
        assert_eq!(store.bytes(), picker("900x600"));
        store.save(b"not a state");
        assert_eq!(store.bytes(), picker("900x600"));
        store.save(&picker("800x500"));
        let path = home.join(FILE);
        let wait = std::time::Instant::now();
        while read(&path) != Some(picker("800x500")) {
            assert!(
                wait.elapsed() < std::time::Duration::from_secs(10),
                "never saved"
            );
            thread::yield_now();
        }
        assert_eq!(Store::open(Some(&home)).bytes(), picker("800x500"));
    }

    #[test]
    fn a_save_keeps_the_keys_it_leaves_out() {
        let store = Store::open(None);
        store.save(&picker("900x600"));
        let mut trash = WindowState::default();
        trash.set("trash", "shown");
        store.save(&trash.to_bytes());
        let saved = WindowState::parse(&store.bytes());
        assert_eq!(saved.get("picker"), Some("900x600"));
        assert_eq!(saved.get("trash"), Some("shown"));
    }
}
