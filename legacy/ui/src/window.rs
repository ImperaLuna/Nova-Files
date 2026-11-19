mod access;
mod acting;
mod aiming;
mod confirm;
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
mod slots;
mod start;
mod tab_order;
mod tabs;
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
pub use slots::from_shell;
pub use tab_order::{DraggedTab, Side as TabSide, TabChip};

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::StreamExt as _;
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, SharedString, Subscription, Task,
    UniformListScrollHandle,
};
use nova_files_api::{
    ClientGone, Command, Connection, Connector, Envelope, Event, EventSink, FolderView,
    HiddenFiles, Listing, ListingId, Order,
};
use nova_files_drives_api as drives_api;
use nova_files_gathered_api::{self as gathered_api, Gathering};
use nova_files_places_api::{self as places_api, Bookmark, Place};
use nova_files_properties_api as properties_api;
use nova_files_settings_api::{self as settings_api, Settings};
use nova_files_shell::Lines;
use nova_files_shell::{
    FolderRules, Host, KnownFolders, Link, Opening, Parts, ThumbnailsShown, WindowParts,
};
use nova_files_start_api::{self as start_api, RecentFolder, RecentId};
use nova_files_transfer_api as transfer_api;

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

const DEFAULT_SCALE_FACTOR: f32 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreStatus {
    Connecting,
    Connected,
    Gone,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowEvent {
    Status(CoreStatus),
    Opened(PathBuf),
    GatheredShown(Gathering),
    StartShown,
    Refused(PathBuf),
    Places,
    Settings,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Went {
    #[default]
    Stayed,
    Elsewhere,
}

pub struct MainWindow {
    status: CoreStatus,
    connector: Connector,
    pub(crate) connection: Option<Arc<Connection>>,
    pub(crate) listing: Option<Listing>,
    details: HashMap<usize, RowRead>,
    theme_icons: ThemeIcons,
    thumbnails: ShownThumbnails,
    pub(crate) scale_factor: f32,
    rows_wanted: Range<usize>,
    pub(crate) rows_drawn: Range<usize>,
    sizing: views::Sizing,
    pub(crate) selection: Option<Selection>,
    pub(crate) jump: TypeJump,
    went: Went,
    pub(crate) quick_looks: QuickLooks,
    pub(crate) band: Option<Band>,
    pub(crate) pressed_name: bool,
    pub(crate) asked: Option<ListingId>,
    requested: Option<Location>,
    order: Order,
    asked_order: Order,
    shown_order: Order,
    core_moved: bool,
    next_listing: u64,
    fallbacks: Vec<PathBuf>,
    pub(crate) falling_back: bool,
    pub(crate) places: Vec<Place>,
    folder_views: HashMap<PathBuf, FolderView>,
    pending_views: HashMap<PathBuf, FolderView>,
    pub(crate) tree: FolderTree,
    tree_line: TreeLine,
    pinned: Arc<[Bookmark]>,
    gathered: gathered::GatheredState,
    pub(crate) history: History,
    pub(crate) recent: Vec<RecentFolder>,
    pub(crate) recent_asked: RecentId,
    pub(crate) path_bar: PathBar,
    pub(crate) scroll: UniformListScrollHandle,
    pub(crate) pick: Option<Picker>,
    pub(crate) home: PathBuf,
    settings: Settings,
    hidden_files: Option<HiddenFiles>,
    thumbnails_shown: ThumbnailsShown,
    known: KnownFolders,
    pub(crate) drives: Drives,
    pub(crate) menu: Option<Menu>,
    pub(crate) search: search::Search,
    pub(crate) focus: FocusHandle,
    pub(crate) island: Island,
    jobs: Jobs,
    pub(crate) counted: SharedString,
    renaming: Option<Renaming>,
    waiting: Waiting,
    showing: Option<showing::Showing>,
    tabs: Tabs,
    drop_asks: aiming::Asks,
    hover_open: hover_open::HoverOpen,
    tab_order: tab_order::TabOrder,
    properties: Option<properties::Page>,
    host: Host,
    pub(crate) parts: WindowParts,
    watching: Vec<Subscription>,
    _events: Task<()>,
}

pub struct ChannelSink<M>(pub UnboundedSender<M>);

impl EventSink for ChannelSink<Event> {
    fn deliver_envelope(&self, envelope: Envelope) -> Result<(), ClientGone> {
        let Some(event) = envelope.taken_as::<Event>() else {
            return Ok(());
        };
        self.0.unbounded_send(event).map_err(|_| ClientGone)
    }
}

impl EventSink for ChannelSink<Envelope> {
    fn deliver_envelope(&self, envelope: Envelope) -> Result<(), ClientGone> {
        self.0.unbounded_send(envelope).map_err(|_| ClientGone)
    }
}

impl MainWindow {
    pub fn new(connector: &Connector, folder: &Path, cx: &mut Context<Self>) -> Self {
        Self::with_home(connector, folder, home_folder(), cx)
    }

    pub fn with_home(
        connector: &Connector,
        folder: &Path,
        home: PathBuf,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::at(connector, &Location::from(folder), home, cx)
    }

    const FIRST_ASKS: [Command; 2] = [Command::Ping, Command::FolderViews];

    pub fn at(
        connector: &Connector,
        location: &Location,
        home: PathBuf,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::opened_with(connector, location, home, Opening::default(), cx)
    }

    pub fn opened_with(
        connector: &Connector,
        location: &Location,
        home: PathBuf,
        opening: Opening,
        cx: &mut Context<Self>,
    ) -> Self {
        let (sink, events) = mpsc::unbounded();
        let connection = connect(connector, ChannelSink(sink)).map(Arc::new);
        let status = if connection.is_some() {
            CoreStatus::Connecting
        } else {
            CoreStatus::Gone
        };
        let events = Self::listen(events, cx);
        let (host, parts) = open_parts(connector, connection.clone(), &home, opening, cx);
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
            went: Went::Stayed,
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
            fallbacks: vec![PathBuf::from(ROOT_FOLDER), home.clone()],
            tree: FolderTree::new(home.clone()),
            falling_back: false,
            home,
            places: Vec::new(),
            folder_views: HashMap::new(),
            pending_views: HashMap::new(),
            tree_line: TreeLine::connect(&host.lines, cx),
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
            thumbnails_shown: ThumbnailsShown::Shown,
            known: KnownFolders::default(),
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
            tabs: Tabs::default(),
            drop_asks: aiming::Asks::default(),
            hover_open: hover_open::HoverOpen::default(),
            tab_order: tab_order::TabOrder::default(),
            properties: None,
            host,
            parts,
            watching: Vec::new(),
            _events: events,
        };
        window.watching = window.watch_slots(cx);
        window.go_first(location, cx);
        window
    }

    fn next_ask(&self) -> u64 {
        self.host.asks.next()
    }

    fn go_first(&mut self, location: &Location, cx: &mut Context<Self>) {
        match location {
            Location::Start => self.open_start(cx),
            Location::Folder(folder) => self.open(folder),
            Location::Gathered(place) => self.open_gathered(*place, cx),
            Location::Properties(paths) => self.arrive_at_properties(paths, cx),
            Location::Page(page) => self.open_page(page.clone(), cx),
        }
    }

    fn listen(mut events: UnboundedReceiver<Envelope>, cx: &Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            while let Some(envelope) = events.next().await {
                if this.update(cx, |this, cx| this.take(envelope, cx)).is_err() {
                    return;
                }
            }
            this.update(cx, |this, cx| this.set_status(CoreStatus::Gone, cx))
                .ok();
        })
    }

    fn take(&mut self, envelope: Envelope, cx: &mut Context<Self>) {
        let envelope = match envelope.open::<Event>() {
            Ok(event) => return self.on_event(event, cx),
            Err(envelope) => envelope,
        };
        let envelope = match envelope.open::<drives_api::Event>() {
            Ok(event) => return self.drive_event(event, cx),
            Err(envelope) => envelope,
        };
        let envelope = match envelope.open::<gathered_api::Event>() {
            Ok(event) => return self.gathered_event(event, cx),
            Err(envelope) => envelope,
        };
        let envelope = match envelope.open::<places_api::Event>() {
            Ok(places_api::Event::Places(places)) => return self.places_arrived(places, cx),
            Ok(places_api::Event::Bookmarks(pinned)) => return self.pinned_arrived(pinned, cx),
            Err(envelope) => envelope,
        };
        let envelope = match envelope.open::<properties_api::Event>() {
            Ok(event) => return self.properties_event(event, cx),
            Err(envelope) => envelope,
        };
        let envelope = match envelope.open::<settings_api::Event>() {
            Ok(settings_api::Event::Settings(settings)) => {
                return self.settings_arrived(settings, cx);
            }
            Err(envelope) => envelope,
        };
        let envelope = match envelope.open::<start_api::Event>() {
            Ok(start_api::Event::RecentFolders { id, folders }) => {
                return self.recent_arrived(id, folders, cx);
            }
            Err(envelope) => envelope,
        };
        let envelope = match envelope.open::<transfer_api::Event>() {
            Ok(transfer_api::Event::DropWould { id, kind }) => {
                return self.drop_would_arrived(id, kind, cx);
            }
            Err(envelope) => envelope,
        };
        if let Err(envelope) = self.parts.deliver(envelope, cx) {
            log::error!("no part takes {envelope:?}");
        }
    }

    pub fn folder_rules(&self) -> FolderRules {
        self.folder().map_or_else(FolderRules::default, |folder| {
            self.host.rules.for_folder(folder)
        })
    }

    pub const fn status(&self) -> CoreStatus {
        self.status
    }

    pub const fn location(&self) -> Option<&Location> {
        self.history.here()
    }

    pub fn is_on_start(&self) -> bool {
        self.location() == Some(&Location::Start)
    }

    pub fn folder(&self) -> Option<&Path> {
        if let Some(from) = self.search.from() {
            return from.folder();
        }
        if self.gathered_shown().is_some() {
            return None;
        }
        self.listing.as_ref().map(|listing| listing.path.as_path())
    }

    pub const fn waiting(&self) -> bool {
        self.asked.is_some()
    }

    pub fn entry_names(&self) -> Vec<String> {
        self.listing.as_ref().map_or_else(Vec::new, |listing| {
            listing
                .entries
                .iter()
                .map(|entry| entry.name.to_string_lossy().into_owned())
                .collect()
        })
    }

    pub const fn scroll(&self) -> &UniformListScrollHandle {
        &self.scroll
    }

    pub fn places(&self) -> &[Place] {
        &self.places
    }

    pub const fn known_folders(&self) -> &KnownFolders {
        &self.known
    }

    pub fn pinned(&self) -> &[Bookmark] {
        &self.pinned
    }

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

fn connect(connector: &Connector, sink: ChannelSink<Envelope>) -> Option<Connection> {
    connector
        .connect(sink)
        .and_then(|connection| {
            for command in MainWindow::FIRST_ASKS {
                connection.send(command)?;
            }
            connection.send(drives_api::Command::Drives)?;
            connection.send(places_api::Command::Places)?;
            connection.send(settings_api::Command::Watch)?;
            Ok(connection)
        })
        .ok()
}

fn open_parts(
    connector: &Connector,
    connection: Option<Arc<Connection>>,
    home: &Path,
    opening: Opening,
    cx: &mut App,
) -> (Host, WindowParts) {
    let host = Host::new(Link::new(connection), home.to_path_buf(), opening, cx)
        .with_lines(Lines::new(Some(connector.clone())));
    let parts = cx
        .try_global::<Parts>()
        .cloned()
        .unwrap_or_default()
        .open(&host, cx);
    (host, parts)
}

pub const ROOT_FOLDER: &str = "/";

pub fn home_folder() -> PathBuf {
    std::env::home_dir().unwrap_or_else(|| PathBuf::from(ROOT_FOLDER))
}
