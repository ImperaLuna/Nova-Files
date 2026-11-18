//! The main window: the places on the left, the open folder's entries (or
//! the Start page) on the right, and an Up button. This file holds the
//! window's state and opening it; the work is split by area: going places
//! in `navigate`, pinning folders in `pin`, the folder tree's own line to
//! the core in `tree`, the drives in `drives`, the Start page in `start`, Games and Projects
//! in `gathered`,
//! the core's events in `events`, the rows on screen in `rows`, the
//! quick look in `preview`, acting on the files selected in `acting` and
//! `clipboard`, dragging and dropping files in `dragging` (naming the
//! drop under the pointer in `aiming`, opening rows held over in
//! `hover_open`), renaming and
//! New folder in `renaming`, the Properties page in `properties` (its
//! size in `measuring`, its app in `opens_with`, who may do what in
//! `access`), the jobs they start in `jobs`, the tabs in `tabs`
//! (reordered in `tab_order`), showing hidden files in `hidden`, the files
//! an app asked to show selected in `showing`, the folders' views in
//! `views`, and the sheet's right-click
//! menus in `context_menu`. Drawing
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
//! folder joins it once its listing is on screen. The Start page needs
//! nothing from the core to show, so going there arrives at once and lets
//! go of the folder's listing (see `start`); a new window opens on it.
//! Each folder that arrives as a new place in a main window is told to the
//! core as used, for the Start page's recent folders. Re-reads and fallbacks
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

mod access;
mod acting;
mod aiming;
mod clipboard;
mod context_menu;
mod dragging;
mod drives;
mod events;
mod gallery;
mod gathered;
mod gathering;
mod hidden;
mod hover_open;
mod jobs;
mod measuring;
mod navigate;
mod opens_with;
mod pin;
mod preview;
mod properties;
mod renaming;
mod rows;
mod search;
mod showing;
mod start;
mod tab_order;
mod tabs;
mod trash;
mod tree;
mod views;

pub use access::{Level as AccessLevel, Who, runs, shared_level};
pub use aiming::Aim;
pub use dragging::DraggedRow;
pub use gathered::LIBRARY_LABEL;
pub use hover_open::{HOVER_OPEN_AFTER, HoverOpens};
pub use jobs::SHOW_AFTER as JOB_SHOWS_AFTER;
pub use navigate::tidy;
pub use preview::{Previewed, QuickLook, Shown};
pub use properties::Page as PropertiesPage;
pub use search::{FocusTo, Scope, short_folder};
pub use tab_order::{DraggedTab, Side as TabSide, TabChip};

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::StreamExt as _;
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, SharedString, Task, UniformListScrollHandle,
};
use nova_files_api::{
    Bookmark, ClientGone, Command, Connection, Connector, Event, EventSink, FolderView, Gathering,
    HiddenFiles, Listing, ListingId, Order, Place, RecentFolder, RecentId, Settings,
};

use self::drives::Drives;
use self::preview::QuickLooks;
use self::rows::RowRead;
use self::tree::TreeLine;
use crate::band::Band;
use crate::history::History;
use crate::island::Island;
use crate::jump::TypeJump;
use crate::location::Location;
use crate::menu::Menu;
use crate::path_bar::PathBar;
use crate::pick::Picker;
use crate::selection::Selection;
use crate::shown_thumbnails::ShownThumbnails;
use crate::theme_icons::ThemeIcons;
use crate::tree::FolderTree;
use jobs::Jobs;
use renaming::{Renaming, Waiting};
use tabs::Tabs;
use trash::TrashState;

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
    /// What Games or Projects gathers is now on screen.
    GatheredShown(Gathering),
    /// The Start page is now on screen.
    StartShown,
    /// The folder asked for couldn't be read; the window stayed put.
    Refused(PathBuf),
    /// The places list or the pinned folders changed.
    Places,
    /// The settings changed.
    Settings,
    /// The theme the app draws with changed.
    Theme,
}

pub struct MainWindow {
    status: CoreStatus,
    /// What connects to the core, for the windows this one opens.
    connector: Connector,
    /// Held so the core counts this window until it closes.
    pub(crate) connection: Option<Connection>,
    /// The folder on screen, once its first listing has arrived.
    pub(crate) listing: Option<Listing>,
    /// The details and icons the core has sent so far, by row. Rows never
    /// shown have none.
    details: HashMap<usize, RowRead>,
    /// The theme icons the rows draw, made at the size they're drawn.
    theme_icons: ThemeIcons,
    /// Thumbnails of the rows in `rows_wanted`, by row.
    thumbnails: ShownThumbnails,
    /// The window's scale factor when it last drew, for sizing thumbnails.
    pub(crate) scale_factor: f32,
    /// The rows the core was last told about: on screen and one screen
    /// either side.
    rows_wanted: Range<usize>,
    /// The rows the list drew last; see [`Self::rows_drawn`].
    pub(crate) rows_drawn: Range<usize>,
    /// What the view's layout keeps between frames.
    sizing: views::Sizing,
    /// What is selected, and the row the keyboard is on (see `selecting`).
    pub(crate) selection: Option<Selection>,
    /// The letters typed to jump to a name (see `jump`).
    pub(crate) jump: TypeJump,
    /// The quick look, while it is open (see `preview`).
    pub(crate) quick_looks: QuickLooks,
    /// A press on the entries list being dragged (see `band`).
    pub(crate) band: Option<Band>,
    /// Whether the press under way began on an entry's name.
    pub(crate) pressed_name: bool,
    /// The listing asked for most recently; only its answer counts.
    pub(crate) asked: Option<ListingId>,
    /// The place asked for most recently: a folder, or Games or Projects.
    requested: Option<Location>,
    /// The order the window asks for listings in (see `navigate`).
    order: Order,
    /// The order of the listing asked for most recently.
    asked_order: Order,
    /// The order of the listing on screen, which the column heads show.
    shown_order: Order,
    /// Whether the core sent a listing since the one on screen, which the
    /// window dropped: the core now serves rows of that one, not this,
    /// until the window asks for the one on screen again.
    core_moved: bool,
    next_listing: u64,
    /// Where to go, in order, while the window has no folder and the one
    /// asked for can't be read.
    fallbacks: Vec<PathBuf>,
    /// Whether the folder asked for is a fallback, which nobody chose, so
    /// it doesn't count as used.
    pub(crate) falling_back: bool,
    pub(crate) places: Vec<Place>,
    /// The view each folder was given, as the core last sent them (see
    /// `views`).
    folder_views: HashMap<PathBuf, FolderView>,
    /// Views given here that the core's list doesn't hold yet.
    pending_views: HashMap<PathBuf, FolderView>,
    /// The sidebar's folder tree under Home, and its line to the core.
    pub(crate) tree: FolderTree,
    tree_line: TreeLine,
    /// The folders the user pinned, in their order.
    pinned: Arc<[Bookmark]>,
    /// Games and Projects, and what the window knows of them (see
    /// `gathered`).
    gathered: gathered::GatheredState,
    /// Where the window has been, for Back and Forward.
    pub(crate) history: History,
    /// The recent folders the Start page shows, newest first, as the core
    /// last sent them (see `start`).
    pub(crate) recent: Vec<RecentFolder>,
    /// The newest ask for the recent folders; only its answer counts.
    pub(crate) recent_asked: RecentId,
    /// The path bar's open menu (see `path_bar`).
    pub(crate) path_bar: PathBar,
    pub(crate) scroll: UniformListScrollHandle,
    /// Another app's request, while the window is its picker (see `pick`).
    pub(crate) pick: Option<Picker>,
    /// The home folder, which `~` names in a picker's name box.
    pub(crate) home: PathBuf,
    /// The settings as the core last sent them.
    settings: Settings,
    /// Whether hidden files show, once the core has said (see `hidden`).
    hidden_files: Option<HiddenFiles>,
    /// The drives and how full they are (see `drives`).
    pub(crate) drives: Drives,
    /// The menu open over the window, if any (see `menu`).
    pub(crate) menu: Option<Menu>,
    /// The search box and the search under way (see `search`).
    pub(crate) search: search::Search,
    /// Focus for the window's own keys (see `commands`).
    pub(crate) focus: FocusHandle,
    /// What the island at the bottom of the sheet says (see `island`).
    pub(crate) island: Island,
    /// The jobs this window started (see `jobs`).
    jobs: Jobs,
    /// The quiet count of the listing on screen, worked out once when it
    /// arrives, so a frame never walks a long folder (see `view::island`).
    pub(crate) counted: SharedString,
    /// The rename under way (see `renaming`).
    renaming: Option<Renaming>,
    /// What a rename or a new folder waits for (see `renaming`).
    waiting: Waiting,
    /// Names to select in the first listing (see `showing`).
    showing: Option<showing::Showing>,
    /// The trash, and what the window knows of it (see `trash`).
    trash: TrashState,
    /// The tabs in the background, and which one is on screen (see
    /// `tabs`).
    tabs: Tabs,
    /// What the window asked the core about drops (see `aiming`).
    drop_asks: aiming::Asks,
    /// The row things are dragged over, which opens if they stay (see
    /// `hover_open`).
    hover_open: hover_open::HoverOpen,
    /// A tab being dragged to another place in the Open list (see
    /// `tab_order`).
    tab_order: tab_order::TabOrder,
    /// The Properties page of the tab on screen, while it has one (see
    /// `properties`).
    properties: Option<properties::Page>,
    /// The id the next properties request or size scan gets.
    next_ask: u64,
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
        let mut window = Self::with_home(connector, folder, home_folder(), cx);
        // `$XDG_DATA_HOME`, when it is an absolute path, holds the trash.
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute());
        window.trash = TrashState::new(&window.home, data);
        window
    }

    /// Opens a window on `folder` that falls back to `home` instead of the
    /// home folder the environment names, so tests never open the real one.
    pub fn with_home(
        connector: &Connector,
        folder: &Path,
        home: PathBuf,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::at(connector, &Location::from(folder), home, cx)
    }

    /// What a window asks the core for as it opens.
    const FIRST_ASKS: [Command; 7] = [
        Command::Ping,
        Command::Places,
        Command::Drives,
        Command::Settings,
        Command::FolderViews,
        Command::Theme,
        Command::ListTrash,
    ];

    /// Opens a window at `location`, with `home` as the home folder.
    pub fn at(
        connector: &Connector,
        location: &Location,
        home: PathBuf,
        cx: &mut Context<Self>,
    ) -> Self {
        let (sink, events) = mpsc::unbounded();
        let connection = connect(connector, ChannelSink(sink));
        let status = if connection.is_some() {
            CoreStatus::Connecting
        } else {
            CoreStatus::Gone
        };
        let events = Self::listen(events, cx);
        let trash = TrashState::new(&home, None);
        let mut window = Self {
            connector: connector.clone(),
            status,
            connection,
            listing: None,
            details: HashMap::new(),
            theme_icons: ThemeIcons::default(),
            thumbnails: ShownThumbnails::default(),
            rows_wanted: 0..0,
            rows_drawn: 0..0,
            sizing: views::Sizing::default(),
            selection: None,
            jump: TypeJump::default(),
            quick_looks: QuickLooks::default(),
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
            tree: FolderTree::new(home.clone()),
            falling_back: false,
            home,
            places: Vec::new(),
            folder_views: HashMap::new(),
            pending_views: HashMap::new(),
            tree_line: TreeLine::connect(connector, cx),
            pinned: Arc::from([]),
            gathered: gathered::GatheredState::default(),
            history: History::default(),
            recent: Vec::new(),
            recent_asked: RecentId(0),
            path_bar: PathBar::new(connector.clone()),
            scroll: UniformListScrollHandle::new(),
            pick: None,
            settings: Settings::default(),
            hidden_files: None,
            drives: Drives::default(),
            menu: None,
            search: search::Search::new(cx),
            focus: cx.focus_handle(),
            island: Island::default(),
            jobs: Jobs::default(),
            counted: SharedString::default(),
            renaming: None,
            waiting: Waiting::Nothing,
            showing: None,
            trash,
            tabs: Tabs::default(),
            drop_asks: aiming::Asks::default(),
            hover_open: hover_open::HoverOpen::default(),
            tab_order: tab_order::TabOrder::default(),
            properties: None,
            next_ask: 0,
            _events: events,
        };
        window.go_first(location, cx);
        window
    }

    /// A new id for a request, to match its answer to it.
    const fn next_ask(&mut self) -> u64 {
        self.next_ask += 1;
        self.next_ask
    }

    /// Goes to `location`, the first place of a new window.
    fn go_first(&mut self, location: &Location, cx: &mut Context<Self>) {
        match location {
            Location::Start => self.open_start(cx),
            Location::Folder(folder) => self.open(folder),
            Location::Gathered(place) => self.open_gathered(*place, cx),
            Location::Properties(paths) => self.arrive_at_properties(paths, cx),
        }
    }

    /// Hands the core's events to the window as they come, until the core
    /// drops them.
    fn listen(mut events: UnboundedReceiver<Event>, cx: &Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
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
        })
    }

    pub const fn status(&self) -> CoreStatus {
        self.status
    }

    /// Where the window is, once it has been anywhere.
    pub const fn location(&self) -> Option<&Location> {
        self.history.here()
    }

    /// Whether the Start page is on screen.
    pub fn is_on_start(&self) -> bool {
        self.location() == Some(&Location::Start)
    }

    /// The folder on screen, if one has arrived yet, none on Start or in
    /// Games or Projects; while a search is under way, the folder it
    /// started in, none when it started in Games or Projects.
    pub fn folder(&self) -> Option<&Path> {
        if let Some(from) = self.search.from() {
            return from.folder();
        }
        if self.gathered_shown().is_some() {
            return None;
        }
        self.listing.as_ref().map(|listing| listing.path.as_path())
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

    /// The folders the user pinned, in their order.
    pub fn pinned(&self) -> &[Bookmark] {
        &self.pinned
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

/// Connects to the core with `sink` for its events, and asks for what
/// every window shows; `None` when the core isn't running.
fn connect(connector: &Connector, sink: ChannelSink) -> Option<Connection> {
    connector
        .connect(sink)
        .and_then(|connection| {
            for command in MainWindow::FIRST_ASKS {
                connection.send(command)?;
            }
            Ok(connection)
        })
        .ok()
}

/// The root folder, where a window lands when nothing else can be read.
pub const ROOT_FOLDER: &str = "/";

/// Where a window opens unless told otherwise: the home folder, or the
/// root folder on a system without one.
pub fn home_folder() -> PathBuf {
    std::env::home_dir().unwrap_or_else(|| PathBuf::from(ROOT_FOLDER))
}
