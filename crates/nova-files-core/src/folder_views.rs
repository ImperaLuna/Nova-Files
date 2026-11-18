//! The view each folder was last given ([`Command::SetFolderView`]), kept
//! between runs and sent to the clients that asked
//! ([`Command::FolderViews`]) whenever one changes.
//!
//! The views live in memory while the core runs and in
//! `$XDG_STATE_HOME/nova-files/folder-views` between runs, newest first,
//! at most [`MAX_FOLDER_VIEWS`]. The file is read once, when the core
//! starts; a missing, unreadable, oversized or malformed one reads as no
//! views. Each change saves the whole list through a [`Saver`].
//!
//! The file starts with a line naming the format and its version. Each
//! folder is then a byte for its mode, a byte for its icon size, its path
//! as raw bytes, and a NUL byte, which no path holds.
//!
//! [`Command::FolderViews`]: nova_files_api::Command::FolderViews
//! [`Command::SetFolderView`]: nova_files_api::Command::SetFolderView

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nova_files_api::{
    ClientId, Event, EventSink, FolderView, FolderViews, IconSize, MAX_FOLDER_VIEWS, ViewMode,
};

use crate::private_file::{self, Read};
use crate::recent::{END, MAX_PATH, fits};
use crate::saver::Saver;

/// The file, under the state folder.
const FILE: &str = "nova-files/folder-views";
/// The first line, naming the format and its version.
const HEADER: &[u8] = b"nova-files folder-views 1\n";
/// The mode and size bytes before each path.
const VIEW_BYTES: usize = 2;
/// A file this big is not one the core wrote; it is never read.
const MAX_BYTES: usize = HEADER.len() + MAX_FOLDER_VIEWS * (VIEW_BYTES + MAX_PATH + 1);
const SAVE_THREAD: &str = "nova-files-views";
/// The file, as the log names it.
const WHAT: &str = "folder views";
/// Each mode's byte in the file.
const DETAILS: u8 = b'd';
const ICONS: u8 = b'i';
const GALLERY: u8 = b'g';
/// Each icon size's byte in the file, smallest first.
const SIZES: [u8; 5] = *b"12345";

/// The folders' views, as the core keeps them.
pub struct Store {
    views: RefCell<FolderViews>,
    listeners: RefCell<BTreeMap<ClientId, Arc<dyn EventSink>>>,
    /// Saves them; to nowhere without a home folder, which keeps them in
    /// memory only.
    saver: Saver,
}

impl Store {
    /// The views saved under the state folder `state_home`.
    pub fn open(state_home: Option<&Path>) -> Self {
        let path = state_home.map(|home| home.join(FILE));
        let views = path.as_deref().map(read).unwrap_or_default();
        Self {
            views: RefCell::new(views.into()),
            listeners: RefCell::new(BTreeMap::new()),
            saver: Saver::new(path, WHAT, SAVE_THREAD),
        }
    }

    /// Sends `client` the views now and after each change.
    pub fn listen(&self, client: ClientId, events: &Arc<dyn EventSink>) {
        let views = Arc::clone(&self.views.borrow());
        if events.deliver(Event::FolderViews(views)).is_ok() {
            self.listeners
                .borrow_mut()
                .insert(client, Arc::clone(events));
        }
    }

    /// Stops sending the views to `client`.
    pub fn forget(&self, client: ClientId) {
        self.listeners.borrow_mut().remove(&client);
    }

    /// Gives `folder` `view`, first in the list, dropping the oldest past
    /// the cap; then tells the listening clients and saves the list. A path
    /// that isn't absolute, or one too long for Linux, is ignored, and so
    /// is the view `folder` already has.
    pub fn set(&self, folder: PathBuf, view: FolderView) {
        if !fits(&folder) {
            return;
        }
        let views = {
            let mut views = self.views.borrow_mut();
            if views
                .iter()
                .any(|(kept, had)| *kept == folder && *had == view)
            {
                return;
            }
            let mut changed: Vec<(PathBuf, FolderView)> = views
                .iter()
                .filter(|(kept, _)| *kept != folder)
                .cloned()
                .collect();
            changed.insert(0, (folder, view));
            changed.truncate(MAX_FOLDER_VIEWS);
            *views = changed.into();
            Arc::clone(&views)
        };
        self.listeners.borrow_mut().retain(|_, events| {
            events
                .deliver(Event::FolderViews(Arc::clone(&views)))
                .is_ok()
        });
        self.saver.save(to_bytes(&views).into());
    }
}

/// The file's bytes for `views`.
fn to_bytes(views: &[(PathBuf, FolderView)]) -> Vec<u8> {
    let mut bytes = HEADER.to_vec();
    for (folder, view) in views {
        bytes.push(mode_byte(view.mode));
        bytes.push(size_byte(view.icons));
        bytes.extend_from_slice(folder.as_os_str().as_bytes());
        bytes.push(END);
    }
    bytes
}

const fn mode_byte(mode: ViewMode) -> u8 {
    match mode {
        ViewMode::Details => DETAILS,
        ViewMode::Icons => ICONS,
        ViewMode::Gallery => GALLERY,
    }
}

const fn size_byte(size: IconSize) -> u8 {
    SIZES[size.index()]
}

/// The view a record's first two bytes name, if they name one.
fn view_of(mode: u8, size: u8) -> Option<FolderView> {
    let mode = match mode {
        DETAILS => ViewMode::Details,
        ICONS => ViewMode::Icons,
        GALLERY => ViewMode::Gallery,
        _ => return None,
    };
    let at = SIZES.iter().position(|byte| *byte == size)?;
    Some(FolderView {
        mode,
        icons: IconSize::ALL[at],
    })
}

/// The views `bytes` hold, or none when they aren't a list the core wrote.
fn parse(bytes: &[u8]) -> Vec<(PathBuf, FolderView)> {
    let Some(body) = bytes.strip_prefix(HEADER) else {
        return Vec::new();
    };
    let Some(body) = body.strip_suffix(&[END]) else {
        return Vec::new();
    };
    let views: Option<Vec<(PathBuf, FolderView)>> = body
        .split(|byte| *byte == END)
        .map(|record| {
            let [mode, size, path @ ..] = record else {
                return None;
            };
            let folder = PathBuf::from(OsString::from_vec(path.to_vec()));
            fits(&folder).then_some((folder, view_of(*mode, *size)?))
        })
        .collect();
    match views {
        Some(views) if views.len() <= MAX_FOLDER_VIEWS => views,
        _ => Vec::new(),
    }
}

fn read(path: &Path) -> Vec<(PathBuf, FolderView)> {
    match private_file::read(path, MAX_BYTES) {
        Read::Bytes(bytes) => parse(&bytes),
        Read::Missing | Read::TooBig | Read::Failed(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests;
