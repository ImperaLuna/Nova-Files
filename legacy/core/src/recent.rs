use std::cell::RefCell;
use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use nova_files_api::{
    EntryKind, Event, EventSink, MAX_RECENT_FOLDERS, PEEK_ENTRIES, Peeked, RecentFolder, RecentId,
};

use crate::list;
use crate::private_file::{self, Read};
use crate::saver::Saver;
use crate::timed;
use nova_files_disk::stored_path::{END, MAX_PATH, fits};

const FILE: &str = "nova-files/recent-folders";
const HEADER: &[u8] = b"nova-files recent-folders 1\n";
const MAX_BYTES: usize = HEADER.len() + MAX_RECENT_FOLDERS * (MAX_PATH + 1);
const SAVE_THREAD: &str = "nova-files-recent";
const READ_THREAD: &str = "nova-files-recent-read";
const LOOK_THREAD: &str = "nova-files-recent-look";
const READ_WAIT: Duration = Duration::from_secs(1);
const WHAT: &str = "recent folders";

pub struct Store {
    folders: RefCell<Vec<PathBuf>>,
    saver: Saver,
}

impl Store {
    pub fn open(state_home: Option<&Path>) -> Self {
        let path = state_home.map(|home| home.join(FILE));
        let read = path.as_deref().map(read);
        let saved_to = path.filter(|_| read.as_ref().is_none_or(Option::is_some));
        Self {
            folders: RefCell::new(read.flatten().unwrap_or_default()),
            saver: Saver::new(saved_to, WHAT, SAVE_THREAD),
        }
    }

    pub fn used(&self, folder: PathBuf) {
        if !fits(&folder) {
            return;
        }
        let bytes = {
            let mut folders = self.folders.borrow_mut();
            if folders.first() == Some(&folder) {
                return;
            }
            folders.retain(|kept| *kept != folder);
            folders.insert(0, folder);
            folders.truncate(MAX_RECENT_FOLDERS);
            to_bytes(&folders)
        };
        self.saver.save(bytes.into());
    }

    pub fn answer(
        &self,
        events: &Arc<dyn EventSink>,
        id: RecentId,
        count: usize,
        skip: Vec<PathBuf>,
    ) {
        let folders = self.folders.borrow().clone();
        let sink = Arc::clone(events);
        let spawned = thread::Builder::new()
            .name(READ_THREAD.to_owned())
            .spawn(move || {
                let found = folders
                    .into_iter()
                    .filter(|folder| !skip.contains(folder))
                    .filter_map(|folder| {
                        timed::within(READ_WAIT, LOOK_THREAD, move || look(&folder)).flatten()
                    })
                    .take(count)
                    .collect();
                sink.deliver(Event::RecentFolders { id, folders: found })
                    .ok();
            });
        if spawned.is_err() {
            let folders = Vec::new();
            events.deliver(Event::RecentFolders { id, folders }).ok();
        }
    }
}

fn look(folder: &Path) -> Option<RecentFolder> {
    let entries = list::read_folder(folder, || false).ok()?;
    let modified = fs::metadata(folder).and_then(|meta| meta.modified()).ok();
    let peek = entries
        .iter()
        .take(PEEK_ENTRIES)
        .map(|entry| Peeked {
            amount: amount(&folder.join(&entry.name), entry.kind),
            name: entry.name.clone(),
            kind: entry.kind,
        })
        .collect();
    Some(RecentFolder {
        path: folder.to_owned(),
        modified,
        entries: u64::try_from(entries.len()).unwrap_or(u64::MAX),
        peek,
    })
}

fn amount(path: &Path, kind: EntryKind) -> Option<u64> {
    match kind {
        EntryKind::File => fs::metadata(path).ok().map(|meta| meta.len()),
        EntryKind::Folder => {
            let count = fs::read_dir(path).ok()?.count();
            u64::try_from(count).ok()
        }
    }
}

fn to_bytes(folders: &[PathBuf]) -> Vec<u8> {
    let mut bytes = HEADER.to_vec();
    for folder in folders {
        bytes.extend_from_slice(folder.as_os_str().as_bytes());
        bytes.push(END);
    }
    bytes
}

fn parse(bytes: &[u8]) -> Vec<PathBuf> {
    let Some(body) = bytes.strip_prefix(HEADER) else {
        return Vec::new();
    };
    let Some(body) = body.strip_suffix(&[END]) else {
        return Vec::new();
    };
    let folders: Vec<PathBuf> = body
        .split(|byte| *byte == END)
        .map(|path| PathBuf::from(OsString::from_vec(path.to_vec())))
        .collect();
    if folders.len() > MAX_RECENT_FOLDERS || !folders.iter().all(|folder| fits(folder)) {
        return Vec::new();
    }
    folders
}

fn read(path: &Path) -> Option<Vec<PathBuf>> {
    match private_file::read(path, MAX_BYTES) {
        Read::Bytes(bytes) => Some(parse(&bytes)),
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
    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn the_file_reads_back_what_was_written() {
        let folders = vec![
            PathBuf::from("/home/me/Projects"),
            PathBuf::from(OsString::from_vec(b"/home/me/\xff odd\nname".to_vec())),
        ];
        assert_eq!(parse(&to_bytes(&folders)), folders);
        assert_eq!(to_bytes(&[]), HEADER);
        assert_eq!(parse(HEADER), Vec::<PathBuf>::new());
    }

    #[test]
    fn anything_else_reads_as_no_folders() {
        let too_many = vec![PathBuf::from("/a"); MAX_RECENT_FOLDERS + 1];
        for bytes in [
            b"".to_vec(),
            b"/a\0".to_vec(),
            b"nova-files recent-folders 2\n/a\0".to_vec(),
            b"nova-files recent-folders 1\n/a".to_vec(),
            b"nova-files recent-folders 1\n\0".to_vec(),
            b"nova-files recent-folders 1\nrelative\0".to_vec(),
            b"nova-files recent-folders 1\n/a\0\0".to_vec(),
            to_bytes(&too_many),
        ] {
            assert_eq!(parse(&bytes), Vec::<PathBuf>::new(), "{bytes:?}");
        }
        let full = vec![PathBuf::from("/a"); MAX_RECENT_FOLDERS];
        assert_eq!(parse(&to_bytes(&full)), full);
        let longest = PathBuf::from(format!("/{}", "a".repeat(MAX_PATH - 1)));
        assert!(fits(&longest));
        let largest = to_bytes(&vec![longest; MAX_RECENT_FOLDERS]);
        assert_eq!(largest.len(), MAX_BYTES);
        assert!(!fits(&PathBuf::from(format!("/{}", "a".repeat(MAX_PATH)))));
    }

    #[test]
    fn a_file_that_cant_be_read_is_never_saved_over() {
        let state = std::env::temp_dir().join(format!("nova-files-recent-{}", std::process::id()));
        fs::remove_dir_all(&state).ok();
        let file = state.join(FILE);
        fs::create_dir_all(&file).unwrap();
        let unreadable = Store::open(Some(&state));
        fs::remove_dir(&file).unwrap();
        fs::write(&file, vec![b'x'; MAX_BYTES + 1]).unwrap();
        let too_big = Store::open(Some(&state));
        fs::write(&file, to_bytes(&paths(&["/a"]))).unwrap();
        let readable = Store::open(Some(&state));

        assert_eq!(unreadable.saver.path(), None);
        assert_eq!(too_big.saver.path(), None);
        assert_eq!(readable.saver.path(), Some(file.as_path()));
        assert_eq!(*readable.folders.borrow(), paths(&["/a"]));
        fs::remove_dir_all(state).ok();
    }

    #[test]
    fn a_used_folder_goes_first_once_and_the_oldest_drop_off() {
        let store = Store::open(None);
        for folder in ["/a", "/b", "/a", "/c", "/c", "relative"] {
            store.used(PathBuf::from(folder));
        }
        assert_eq!(*store.folders.borrow(), paths(&["/c", "/a", "/b"]));
        for number in 0..=MAX_RECENT_FOLDERS {
            store.used(PathBuf::from(format!("/{number}")));
        }
        let folders = store.folders.borrow();
        assert_eq!(folders.len(), MAX_RECENT_FOLDERS);
        assert_eq!(folders[0], PathBuf::from(format!("/{MAX_RECENT_FOLDERS}")));
        assert_eq!(folders[MAX_RECENT_FOLDERS - 1], PathBuf::from("/1"));
    }
}
