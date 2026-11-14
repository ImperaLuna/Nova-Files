//! The folders a person opened lately ([`Command::FolderUsed`]), kept for
//! the Start page between runs, and read for [`Command::RecentFolders`].
//!
//! The list lives in memory while the core runs and in
//! `$XDG_STATE_HOME/nova-files/recent-folders` between runs, newest first,
//! at most [`MAX_RECENT_FOLDERS`]. The file is read once, when the core
//! starts; a missing, unreadable, oversized or malformed one reads as no
//! folders. Each change saves the whole list through a [`Saver`].
//!
//! The file starts with a line naming the format and its version, then
//! holds each folder's path as raw bytes, ended by a NUL byte, which no
//! path holds.
//!
//! An answer reads each folder on a thread of its own, so a drive that
//! stopped answering never holds up the core: whether the folder is still
//! there, what it holds and a peek at its first entries. A folder that
//! can't be read is left out of the answer but kept in the list, since its
//! drive may come back.
//!
//! [`Command::FolderUsed`]: nova_files_api::Command::FolderUsed
//! [`Command::RecentFolders`]: nova_files_api::Command::RecentFolders

use std::cell::RefCell;
use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use nova_files_api::{
    EntryKind, Event, EventSink, MAX_RECENT_FOLDERS, PEEK_ENTRIES, Peeked, RecentFolder,
};

use crate::list;
use crate::private_file::{self, Read};
use crate::saver::Saver;

/// The file, under the state folder.
const FILE: &str = "nova-files/recent-folders";
/// The first line, naming the format and its version.
const HEADER: &[u8] = b"nova-files recent-folders 1\n";
/// Ends each path.
const END: u8 = 0;
/// The longest path Linux takes, in bytes.
const MAX_PATH: usize = 4096;
/// A file this big is not one the core wrote; it is never read.
const MAX_BYTES: usize = HEADER.len() + MAX_RECENT_FOLDERS * (MAX_PATH + 1);
const SAVE_THREAD: &str = "nova-files-recent";
const READ_THREAD: &str = "nova-files-recent-read";
/// The file, as the log names it.
const WHAT: &str = "recent folders";

/// The recent folders, as the core keeps them.
pub struct Store {
    /// Newest first.
    folders: RefCell<Vec<PathBuf>>,
    /// Saves them; to nowhere without a home folder, which keeps them in
    /// memory only.
    saver: Saver,
}

impl Store {
    /// The folders saved under the state folder `state_home`.
    pub fn open(state_home: Option<&Path>) -> Self {
        let path = state_home.map(|home| home.join(FILE));
        let folders = path.as_deref().map(read).unwrap_or_default();
        Self {
            folders: RefCell::new(folders),
            saver: Saver::new(path, WHAT, SAVE_THREAD),
        }
    }

    /// Puts `folder` first, dropping the oldest past the cap, and saves the
    /// list. A path that isn't absolute, or one too long for Linux, is
    /// ignored.
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

    /// Sends `events` the newest `count` folders that are still there, can
    /// be read and aren't in `skip`, read on a thread of their own.
    pub fn answer(&self, events: &Arc<dyn EventSink>, count: usize, skip: Vec<PathBuf>) {
        let folders = self.folders.borrow().clone();
        let sink = Arc::clone(events);
        let spawned = thread::Builder::new()
            .name(READ_THREAD.to_owned())
            .spawn(move || {
                let found = folders
                    .iter()
                    .filter(|folder| !skip.contains(folder))
                    .filter_map(|folder| look(folder))
                    .take(count)
                    .collect();
                sink.deliver(Event::RecentFolders(found)).ok();
            });
        if spawned.is_err() {
            // Nothing could be read; the client can ask again.
            events.deliver(Event::RecentFolders(Vec::new())).ok();
        }
    }
}

/// Whether `folder` can go in the list: absolute, and short enough.
fn fits(folder: &Path) -> bool {
    let bytes = folder.as_os_str().as_bytes();
    folder.is_absolute() && bytes.len() <= MAX_PATH && !bytes.contains(&END)
}

/// `folder` as the Start page shows it, or `None` when it isn't a folder
/// that can be read.
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

/// A file's size, or how many entries a folder holds.
fn amount(path: &Path, kind: EntryKind) -> Option<u64> {
    match kind {
        EntryKind::File => fs::metadata(path).ok().map(|meta| meta.len()),
        EntryKind::Folder => {
            let count = fs::read_dir(path).ok()?.count();
            u64::try_from(count).ok()
        }
    }
}

/// The file's bytes for `folders`.
fn to_bytes(folders: &[PathBuf]) -> Vec<u8> {
    let mut bytes = HEADER.to_vec();
    for folder in folders {
        bytes.extend_from_slice(folder.as_os_str().as_bytes());
        bytes.push(END);
    }
    bytes
}

/// The folders `bytes` hold, or none when they aren't a list the core
/// wrote.
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

/// The folders in the file at `path`, or none when it is missing,
/// unreadable, oversized or malformed.
fn read(path: &Path) -> Vec<PathBuf> {
    match private_file::read(path, MAX_BYTES) {
        Read::Bytes(bytes) => parse(&bytes),
        Read::Missing | Read::TooBig | Read::Failed(_) => Vec::new(),
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
        let longest = PathBuf::from(format!("/{}", "a".repeat(MAX_PATH - 1)));
        assert!(fits(&longest));
        assert!(!fits(&PathBuf::from(format!("/{}", "a".repeat(MAX_PATH)))));
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
