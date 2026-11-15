//! The main window: the places on the left, the open folder's entries on
//! the right, and an Up button. This file holds the window's state and
//! opening it; the work is split by area: going places in `navigate`, the
//! core's events in `events`, and the rows on screen in `rows`. Drawing
//! lives in `view`; the actions a key or button sends are handled in
//! `view` too, each by one method on the window (see `commands`).
//!
//! The window never touches the disk. Going somewhere sends the core a
//! `ListFolder` with a new id and keeps showing the current folder until
//! the answer for that id arrives; answers for older ids are dropped. A
//! folder that can't be read leaves the window where it was; if the window
//! has no folder yet, it falls back to the home folder, then the root. The
//! core serves rows of the last listing it sent, so when the window dropped
//! a newer listing it no longer wanted, the folder on screen is read again
//! (once, and not when it is the folder just refused) to get its rows
//! served: at the refusal, or at once if the refusal came first.
//! Back and Forward follow the window's history (see `history`); a
//! folder joins it once its listing is on screen. Re-reads and fallbacks
//! leave the history alone. Paths are tidied by their text alone
//! (`a/../b` is `b`), as a user reads them, so Up goes where the path on
//! screen says.
//!
//! The core's events arrive through an async channel the window's own task
//! reads; closing the window drops the connection, which tells the core
//! one client fewer is using it.
//!
//! As the list draws, the window tells the core which rows are on screen,
//! plus one screen either side. The core answers with those rows' details
//! (read once each; the window keeps them for the listing) and their
//! thumbnails. The window keeps only the thumbnails of those rows, shrunk
//! to the size they're drawn (see `thumbnail`). A thumbnail it lets go of
//! is dropped from GPUI's image atlas too (see `shown_thumbnails`).
//!
//! A search puts the files it found in the list's place (see `search`).

mod events;
mod navigate;
mod rows;
mod search;

pub use navigate::tidy;
pub use search::FocusTo;

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use futures::StreamExt as _;
use futures::channel::mpsc::{self, UnboundedSender};
use gpui::{App, Context, EventEmitter, FocusHandle, Focusable, Task, UniformListScrollHandle};
use nova_files_api::{
    ClientGone, Command, Connection, Connector, Details, Event, EventSink, Listing, ListingId,
    Order, Place, Settings,
};

use crate::band::Band;
use crate::history::History;
use crate::jump::TypeJump;
use crate::path_bar::PathBar;
use crate::pick::Picker;
use crate::selection::Selection;
use crate::shown_thumbnails::ShownThumbnails;

/// Until the window first draws, thumbnails are sized for a 2x screen,
/// which is never too small.
const DEFAULT_SCALE_FACTOR: f32 = 2.0;

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
    /// The settings changed.
    Settings,
    /// The theme the app draws with changed.
    Theme,
}

pub struct MainWindow {
    status: CoreStatus,
    /// Held so the core counts this window until it closes.
    pub(crate) connection: Option<Connection>,
    /// The folder on screen, once its first listing has arrived.
    pub(crate) listing: Option<Listing>,
    /// The details the core has read so far, by row. Rows never shown have
    /// none.
    details: HashMap<usize, Details>,
    /// Thumbnails of the rows in `rows_wanted`, by row.
    thumbnails: ShownThumbnails,
    /// The window's scale factor when it last drew, for sizing thumbnails.
    scale_factor: f32,
    /// The rows the core was last told about: on screen and one screen
    /// either side.
    rows_wanted: Range<usize>,
    /// The rows the list drew last; see [`Self::rows_drawn`].
    pub(crate) rows_drawn: Range<usize>,
    /// What is selected, and the row the keyboard is on (see `selecting`).
    pub(crate) selection: Option<Selection>,
    /// The letters typed to jump to a name (see `jump`).
    pub(crate) jump: TypeJump,
    /// A press on the entries list being dragged (see `band`).
    pub(crate) band: Option<Band>,
    /// Whether the press under way began on an entry's name.
    pub(crate) pressed_name: bool,
    /// The listing asked for most recently; only its answer counts.
    pub(crate) asked: Option<ListingId>,
    /// The folder asked for most recently.
    requested: Option<PathBuf>,
    /// The order the window asks for listings in (see `navigate`).
    order: Order,
    /// The order of the listing asked for most recently.
    asked_order: Order,
    /// The order of the listing on screen, which the column heads show.
    shown_order: Order,
    /// Whether the core sent a listing since the one on screen, which the
    /// window dropped: the core now serves rows of that one, not this.
    core_moved: bool,
    next_listing: u64,
    /// Where to go, in order, while the window has no folder and the one
    /// asked for can't be read.
    fallbacks: Vec<PathBuf>,
    pub(crate) places: Vec<Place>,
    /// Where the window has been, for Back and Forward.
    pub(crate) history: History,
    /// The path bar's open menu (see `path_bar`).
    pub(crate) path_bar: PathBar,
    pub(crate) scroll: UniformListScrollHandle,
    /// Another app's request, while the window is its picker (see `pick`).
    pub(crate) pick: Option<Picker>,
    /// The home folder, which `~` names in a picker's name box.
    pub(crate) home: PathBuf,
    /// The settings as the core last sent them.
    settings: Settings,
    /// The search box and the search under way (see `search`).
    pub(crate) search: search::Search,
    /// Focus for the window's own keys (see `commands`).
    pub(crate) focus: FocusHandle,
    _events: Task<()>,
}

/// Delivers the core's events into an async channel: the window's, or a
/// path bar menu's.
pub struct ChannelSink(pub UnboundedSender<Event>);

impl EventSink for ChannelSink {
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
            .connect(ChannelSink(sink))
            .and_then(|connection| {
                connection.send(Command::Ping)?;
                connection.send(Command::Places)?;
                connection.send(Command::Settings)?;
                connection.send(Command::Theme)?;
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
            details: HashMap::new(),
            thumbnails: ShownThumbnails::default(),
            rows_wanted: 0..0,
            rows_drawn: 0..0,
            selection: None,
            jump: TypeJump::default(),
            band: None,
            pressed_name: false,
            scale_factor: DEFAULT_SCALE_FACTOR,
            asked: None,
            requested: None,
            order: Order::default(),
            asked_order: Order::default(),
            shown_order: Order::default(),
            core_moved: false,
            next_listing: 0,
            // Popped from the end: home first, then the root.
            fallbacks: vec![PathBuf::from(ROOT_FOLDER), home.clone()],
            home,
            places: Vec::new(),
            history: History::default(),
            path_bar: PathBar::new(connector.clone()),
            scroll: UniformListScrollHandle::new(),
            pick: None,
            settings: Settings::default(),
            search: search::Search::new(cx),
            focus: cx.focus_handle(),
            _events: events,
        };
        window.open(folder);
        window
    }

    pub const fn status(&self) -> CoreStatus {
        self.status
    }

    /// The folder on screen, if one has arrived yet; while a search is
    /// under way, the folder it started in.
    pub fn folder(&self) -> Option<&Path> {
        self.search
            .from()
            .or_else(|| self.listing.as_ref().map(|listing| listing.path.as_path()))
    }

    /// Whether a folder asked for hasn't answered yet.
    pub const fn waiting(&self) -> bool {
        self.asked.is_some()
    }

    /// The names on screen, in order: a folder's entries, or the files a
    /// search found.
    pub fn entry_names(&self) -> Vec<String> {
        self.listing.as_ref().map_or_else(Vec::new, |listing| {
            listing
                .entries
                .iter()
                .map(|entry| entry.name.to_string_lossy().into_owned())
                .collect()
        })
    }

    /// The scroll position of the list on screen.
    pub const fn scroll(&self) -> &UniformListScrollHandle {
        &self.scroll
    }

    pub fn places(&self) -> &[Place] {
        &self.places
    }

    /// The settings as the core last sent them.
    pub const fn settings(&self) -> &Settings {
        &self.settings
    }
}

impl EventEmitter<WindowEvent> for MainWindow {}

impl Focusable for MainWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The root folder, where a window lands when nothing else can be read.
pub const ROOT_FOLDER: &str = "/";

/// Where a window opens unless told otherwise: the home folder, or the
/// root folder on a system without one.
pub fn home_folder() -> PathBuf {
    std::env::home_dir().unwrap_or_else(|| PathBuf::from(ROOT_FOLDER))
}
