//! The main window: the places on the left, the open folder's entries on
//! the right, and an Up button.
//!
//! The window never touches the disk. Going somewhere sends the core a
//! `ListFolder` with a new id and keeps showing the current folder until
//! the answer for that id arrives; answers for older ids are dropped. A
//! folder that can't be read leaves the window where it was (reading it
//! again if its details were still coming, since asking cancelled them); if
//! the window has no folder yet, it falls back to the home folder, then the
//! root.
//! Paths are tidied by their text alone (`a/../b` is `b`), as a user reads
//! them, so Up goes where the path on screen says.
//!
//! The core's events arrive through an async channel the window's own task
//! reads; closing the window drops the connection, which tells the core
//! one client fewer is using it. Drawing lives in `view`.

use std::path::{Component, Path, PathBuf};

use futures::StreamExt as _;
use futures::channel::mpsc::{self, UnboundedSender};
use gpui::{Context, EventEmitter, Task, UniformListScrollHandle};
use nova_files_api::{
    ClientGone, Command, Connection, Connector, Details, EntryId, EntryKind, Event, EventSink,
    Listing, ListingId, Place,
};

use crate::columns::{self, ColumnText};

/// What the window knows about the core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreStatus {
    /// Connected, waiting for the first answer.
    Connecting,
    /// The core has answered.
    Connected,
    /// The core is not running.
    Gone,
}

/// What the window tells whoever watches it (tests, for now).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowEvent {
    Status(CoreStatus),
    /// A folder's listing is now on screen.
    Opened(PathBuf),
    /// The folder asked for couldn't be read; the window stayed put.
    Refused(PathBuf),
    /// The places list changed.
    Places,
}

pub struct MainWindow {
    status: CoreStatus,
    /// Held so the core counts this window until it closes.
    connection: Option<Connection>,
    /// The folder on screen, once its first listing has arrived.
    pub(crate) listing: Option<Listing>,
    /// The details of the listing's first entries, in entry order; the
    /// rest are still being read.
    pub(crate) details: Vec<Details>,
    /// The listing asked for most recently; only its answer counts.
    asked: Option<ListingId>,
    next_listing: u64,
    /// Where to go, in order, while the window has no folder and the one
    /// asked for can't be read.
    fallbacks: Vec<PathBuf>,
    pub(crate) places: Vec<Place>,
    pub(crate) scroll: UniformListScrollHandle,
    _events: Task<()>,
}

/// Delivers the core's events into the window's async channel.
struct WindowSink(UnboundedSender<Event>);

impl EventSink for WindowSink {
    fn deliver(&self, event: Event) -> Result<(), ClientGone> {
        self.0.unbounded_send(event).map_err(|_| ClientGone)
    }
}

impl MainWindow {
    /// Opens a window on `folder`.
    pub fn new(connector: &Connector, folder: &Path, cx: &mut Context<Self>) -> Self {
        Self::with_home(connector, folder, home_folder(), cx)
    }

    /// Opens a window on `folder` that falls back to `home` instead of the
    /// home folder the environment names, so tests never open the real one.
    pub fn with_home(
        connector: &Connector,
        folder: &Path,
        home: PathBuf,
        cx: &mut Context<Self>,
    ) -> Self {
        let (sink, mut events) = mpsc::unbounded();
        let connection = connector
            .connect(WindowSink(sink))
            .and_then(|connection| {
                connection.send(Command::Ping)?;
                connection.send(Command::Places)?;
                Ok(connection)
            })
            .ok();
        let status = if connection.is_some() {
            CoreStatus::Connecting
        } else {
            CoreStatus::Gone
        };
        let events = cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this
                    .update(cx, |this, cx| this.on_event(event, cx))
                    .is_err()
                {
                    return;
                }
            }
            // The core dropped this window's events: it has stopped.
            this.update(cx, |this, cx| this.set_status(CoreStatus::Gone, cx))
                .ok();
        });
        let mut window = Self {
            status,
            connection,
            listing: None,
            details: Vec::new(),
            asked: None,
            next_listing: 0,
            // Popped from the end: home first, then the root.
            fallbacks: vec![PathBuf::from(ROOT_FOLDER), home],
            places: Vec::new(),
            scroll: UniformListScrollHandle::new(),
            _events: events,
        };
        window.open(folder);
        window
    }

    pub const fn status(&self) -> CoreStatus {
        self.status
    }

    /// The folder on screen, if one has arrived yet.
    pub fn folder(&self) -> Option<&Path> {
        self.listing.as_ref().map(|listing| listing.path.as_path())
    }

    /// Whether a folder asked for hasn't answered yet.
    pub const fn waiting(&self) -> bool {
        self.asked.is_some()
    }

    /// The names on screen, in order.
    pub fn entry_names(&self) -> Vec<String> {
        self.listing.as_ref().map_or_else(Vec::new, |listing| {
            listing
                .entries
                .iter()
                .map(|entry| entry.name.to_string_lossy().into_owned())
                .collect()
        })
    }

    /// The Modified, Type and Size text of entry `index` on screen. Modified
    /// and Size are empty until the core has read that entry's details.
    pub fn columns(&self, index: usize) -> Option<ColumnText> {
        let entry = self.listing.as_ref()?.entries.get(index)?;
        let details = self.details.get(index).copied().unwrap_or_default();
        Some(ColumnText {
            modified: details.modified.map(columns::modified).unwrap_or_default(),
            type_name: columns::type_name(&entry.name, entry.kind),
            size: details.size.map(columns::size).unwrap_or_default(),
        })
    }

    pub fn places(&self) -> &[Place] {
        &self.places
    }

    /// Asks the core for `folder`; the window moves there once it answers.
    pub fn open(&mut self, folder: &Path) {
        let id = ListingId(self.next_listing);
        self.next_listing += 1;
        let path = tidy(folder);
        let sent = self
            .connection
            .as_ref()
            .map(|connection| connection.send(Command::ListFolder { id, path }));
        if matches!(sent, Some(Ok(()))) {
            self.asked = Some(id);
        }
    }

    /// Goes into the entry `id` of the folder on screen, if it is a folder.
    pub fn open_entry(&mut self, id: EntryId) {
        let Some(listing) = &self.listing else { return };
        let entry = usize::try_from(id.0)
            .ok()
            .and_then(|index| listing.entries.get(index));
        if let Some(entry) = entry
            && entry.kind == EntryKind::Folder
        {
            let folder = listing.path.join(&entry.name);
            self.open(&folder);
        }
    }

    /// The folder Up goes to, if there is one.
    pub fn parent(&self) -> Option<PathBuf> {
        self.folder()?.parent().map(Path::to_path_buf)
    }

    /// Goes to the parent of the folder on screen.
    pub fn go_up(&mut self) {
        if let Some(parent) = self.parent() {
            self.open(&parent);
        }
    }

    fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Pong => self.set_status(CoreStatus::Connected, cx),
            Event::FolderListed(listing) if self.asked == Some(listing.id) => {
                self.asked = None;
                let path = listing.path.clone();
                // Read again in place: keep the scroll position.
                let same_folder = self.folder() == Some(path.as_path());
                self.listing = Some(listing);
                self.details = Vec::new();
                if !same_folder {
                    self.scroll = UniformListScrollHandle::new();
                }
                cx.emit(WindowEvent::Opened(path));
                cx.notify();
            }
            Event::FolderUnreadable { id, path, .. } if self.asked == Some(id) => {
                self.asked = None;
                match &self.listing {
                    None => {
                        if let Some(fallback) = self.fallbacks.pop() {
                            self.open(&fallback);
                        }
                    }
                    // Asking cancelled the details still on their way, so
                    // the folder on screen is read again, once: not when it
                    // is the folder that was just refused.
                    Some(listing)
                        if self.details.len() < listing.entries.len() && listing.path != path =>
                    {
                        let shown = listing.path.clone();
                        self.open(&shown);
                    }
                    Some(_) => {}
                }
                cx.emit(WindowEvent::Refused(path));
            }
            Event::DetailsRead { id, first, details }
                if self.listing.as_ref().map(|listing| listing.id) == Some(id)
                    && usize::try_from(first.0).ok() == Some(self.details.len()) =>
            {
                self.details.extend_from_slice(&details);
                cx.notify();
            }
            // An answer for a folder the window has since moved away from.
            Event::FolderListed(_) | Event::FolderUnreadable { .. } | Event::DetailsRead { .. } => {
            }
            Event::Places(places) => {
                self.places = places;
                cx.emit(WindowEvent::Places);
                cx.notify();
            }
        }
    }

    fn set_status(&mut self, status: CoreStatus, cx: &mut Context<Self>) {
        if self.status != status {
            self.status = status;
            cx.emit(WindowEvent::Status(status));
            cx.notify();
        }
    }
}

impl EventEmitter<WindowEvent> for MainWindow {}

const ROOT_FOLDER: &str = "/";

/// Where a window opens unless told otherwise: the home folder, or the
/// root folder on a system without one.
pub fn home_folder() -> PathBuf {
    std::env::home_dir().unwrap_or_else(|| PathBuf::from(ROOT_FOLDER))
}

/// `path` with `.` dropped and each `..` taking off the part before it,
/// worked out from the text alone. `..` at the root stays at the root.
fn tidy(path: &Path) -> PathBuf {
    let mut tidy = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => match tidy.components().next_back() {
                Some(Component::Normal(_)) => {
                    tidy.pop();
                }
                // `..` at the root stays at the root.
                Some(Component::RootDir | Component::Prefix(_)) => {}
                // Nothing left to take off: the `..` stays.
                Some(Component::ParentDir | Component::CurDir) | None => tidy.push(part),
            },
            Component::RootDir | Component::Prefix(_) | Component::Normal(_) => tidy.push(part),
        }
    }
    if tidy.as_os_str().is_empty() {
        PathBuf::from(CURRENT_FOLDER)
    } else {
        tidy
    }
}

/// The relative path that names the folder it is relative to.
const CURRENT_FOLDER: &str = ".";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tidy_resolves_dots_by_text() {
        assert_eq!(tidy(Path::new("/home/me/..")), PathBuf::from("/home"));
        assert_eq!(tidy(Path::new("/a/./b/../c")), PathBuf::from("/a/c"));
        assert_eq!(tidy(Path::new("/..")), PathBuf::from("/"));
        assert_eq!(tidy(Path::new("/")), PathBuf::from("/"));
        // A relative path keeps the `..` it can't take off.
        assert_eq!(tidy(Path::new("..")), PathBuf::from(".."));
        assert_eq!(tidy(Path::new("../../child")), PathBuf::from("../../child"));
        assert_eq!(tidy(Path::new("a/../../b")), PathBuf::from("../b"));
        assert_eq!(tidy(Path::new("a/..")), PathBuf::from("."));
        assert_eq!(tidy(Path::new(".")), PathBuf::from("."));
    }
}
