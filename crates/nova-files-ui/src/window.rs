//! The main window: the places on the left, the open folder's entries on
//! the right, and an Up button.
//!
//! The window never touches the disk. Going somewhere sends the core a
//! `ListFolder` with a new id and keeps showing the current folder until
//! the answer for that id arrives; answers for older ids are dropped. A
//! folder that can't be read leaves the window where it was.
//!
//! The core's events arrive through an async channel the window's own task
//! reads; closing the window drops the connection, which tells the core
//! one client fewer is using it. Drawing lives in `view`.

use std::path::{Path, PathBuf};

use futures::StreamExt as _;
use futures::channel::mpsc::{self, UnboundedSender};
use gpui::{Context, EventEmitter, Task, UniformListScrollHandle};
use nova_files_api::{
    ClientGone, Command, Connection, Connector, EntryId, EntryKind, Event, EventSink, Listing,
    ListingId, Place,
};

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
    /// The listing asked for most recently; only its answer counts.
    asked: Option<ListingId>,
    next_listing: u64,
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
    pub fn new(connector: &Connector, folder: PathBuf, cx: &mut Context<Self>) -> Self {
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
            asked: None,
            next_listing: 0,
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

    /// Whether a folder was asked for and hasn't answered yet.
    pub const fn is_loading(&self) -> bool {
        self.asked.is_some()
    }

    pub fn places(&self) -> &[Place] {
        &self.places
    }

    /// Asks the core for `folder`; the window moves there once it answers.
    pub fn open(&mut self, folder: PathBuf) {
        let id = ListingId(self.next_listing);
        self.next_listing += 1;
        let sent = self
            .connection
            .as_ref()
            .map(|connection| connection.send(Command::ListFolder { id, path: folder }));
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
            self.open(folder);
        }
    }

    /// The folder Up goes to, if there is one.
    pub fn parent(&self) -> Option<PathBuf> {
        self.folder()?.parent().map(Path::to_path_buf)
    }

    /// Goes to the parent of the folder on screen.
    pub fn go_up(&mut self) {
        if let Some(parent) = self.parent() {
            self.open(parent);
        }
    }

    fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Pong => self.set_status(CoreStatus::Connected, cx),
            Event::FolderListed(listing) if self.asked == Some(listing.id) => {
                self.asked = None;
                let path = listing.path.clone();
                self.listing = Some(listing);
                self.scroll = UniformListScrollHandle::new();
                cx.emit(WindowEvent::Opened(path));
                cx.notify();
            }
            Event::FolderUnreadable { id, path, .. } if self.asked == Some(id) => {
                self.asked = None;
                cx.emit(WindowEvent::Refused(path));
            }
            // An answer for a folder the window has since moved away from.
            Event::FolderListed(_) | Event::FolderUnreadable { .. } => {}
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
