use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nova_files_api::{ClientId, EventSink};
use nova_files_folders_api::{
    Event, FolderView, FolderViews, IconSize, MAX_FOLDER_VIEWS, ViewMode,
};

use crate::private_file::{self, Read};
use crate::recent::{END, MAX_PATH, fits};
use crate::saver::Saver;

const FILE: &str = "nova-files/folder-views";
const HEADER: &[u8] = b"nova-files folder-views 1\n";
const VIEW_BYTES: usize = 2;
const MAX_BYTES: usize = HEADER.len() + MAX_FOLDER_VIEWS * (VIEW_BYTES + MAX_PATH + 1);
const SAVE_THREAD: &str = "nova-files-views";
const WHAT: &str = "folder views";
const DETAILS: u8 = b'd';
const ICONS: u8 = b'i';
const GALLERY: u8 = b'g';
const SIZES: [u8; 5] = *b"12345";

pub struct Store {
    views: RefCell<FolderViews>,
    listeners: RefCell<BTreeMap<ClientId, Arc<dyn EventSink>>>,
    saver: Saver,
}

impl Store {
    pub fn open(state_home: Option<&Path>) -> Self {
        let path = state_home.map(|home| home.join(FILE));
        let views = path.as_deref().map(read).unwrap_or_default();
        Self {
            views: RefCell::new(views.into()),
            listeners: RefCell::new(BTreeMap::new()),
            saver: Saver::new(path, WHAT, SAVE_THREAD),
        }
    }

    pub fn listen(&self, client: ClientId, events: &Arc<dyn EventSink>) {
        let views = Arc::clone(&self.views.borrow());
        if events.deliver(Event::Views(views)).is_ok() {
            self.listeners
                .borrow_mut()
                .insert(client, Arc::clone(events));
        }
    }

    pub fn forget(&self, client: ClientId) {
        self.listeners.borrow_mut().remove(&client);
    }

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
        self.listeners
            .borrow_mut()
            .retain(|_, events| events.deliver(Event::Views(Arc::clone(&views))).is_ok());
        self.saver.save(to_bytes(&views).into());
    }
}

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
