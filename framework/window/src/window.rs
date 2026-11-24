mod acting;
mod aiming;
mod confirm;
mod context_menu;
mod drag_pile;
mod dragging;
mod events;
mod gallery;
mod hidden;
mod hover_open;
mod jobs;
mod navigate;
mod pin;
mod question;
mod renaming;
mod rows;
mod search;
mod shares;
mod shortcuts;
mod showing;
mod slots;
mod start;
mod tab_order;
mod tabs;
mod views;

pub use aiming::Aim;
pub use dragging::DraggedRow;
pub use hover_open::{HOVER_OPEN_AFTER, HoverOpens};
pub use jobs::SHOW_AFTER as JOB_SHOWS_AFTER;
pub use navigate::tidy;
pub use search::{FocusTo, short_folder};
pub use tab_order::{DraggedTab, Side as TabSide, TabChip};

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::StreamExt as _;
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, SharedString,
    Subscription, Task, UniformListScrollHandle,
};
use nova_files_api::{
    ClientGone, Command, Connection, Connector, EntryId, Envelope, Event, EventSink, FolderSizes,
    HiddenFiles, Listing, ListingId, Order,
};
use nova_files_shell::Lines;
use nova_files_shell::path_bar::PathBar;
use nova_files_shell::{
    FolderRules, Host, KnownFolders, Link, Opening, PageRef, Parts, TabId, ThumbnailsShown,
    WindowParts,
};

use self::rows::RowRead;
use crate::band::Band;
use crate::edge_scroll::EdgeScroll;
use crate::grip;
use crate::island::Island;
use crate::jump::TypeJump;
use crate::lift::Lift;
use crate::menu::Menu;
use crate::scroll_bar::ScrollBar;
use crate::selection::Selection;
use crate::shortcuts::Shortcuts;
use crate::theme_icons::ThemeIcons;
use jobs::Jobs;
use nova_files_shell::History;
use nova_files_shell::Location;
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
    StartShown,
    Refused(PathBuf),
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
    pub(crate) connection: Option<Arc<Connection>>,
    pub(crate) listing: Option<Listing>,
    details: HashMap<usize, RowRead>,
    shares: shares::Shares,
    theme_icons: ThemeIcons,
    pub(crate) scale_factor: f32,
    rows_wanted: Range<usize>,
    pub(crate) rows_drawn: Range<usize>,
    sizing: views::Sizing,
    pub(crate) selection: Option<Selection>,
    pub(crate) jump: TypeJump,
    went: Went,
    pub(crate) band: Option<Band>,
    pub(crate) pressed_name: bool,
    pub(crate) lift: Option<Lift>,
    pub(crate) edge: EdgeScroll,
    pub(crate) bar: ScrollBar,
    pub(crate) grip: Option<grip::Held>,
    pub(crate) selected_on_press: Option<(ListingId, EntryId)>,
    pub(crate) selection_before_press: Option<Selection>,
    pub(crate) pressed_text: Option<(ListingId, EntryId)>,
    pub(crate) slow_rename: Option<Task<()>>,
    pub(crate) asked: Option<ListingId>,
    requested: Option<Location>,
    order: Order,
    asked_order: Order,
    shown_order: Order,
    core_moved: bool,
    next_listing: u64,
    fallbacks: Vec<PathBuf>,
    pub(crate) falling_back: bool,
    pub(crate) history: History<Location>,
    pub(crate) path_bar: Entity<PathBar>,
    pub(crate) asks_sent: u64,
    pub(crate) scroll: UniformListScrollHandle,
    pub(crate) bounds_watch: Option<Subscription>,
    pub(crate) home: PathBuf,
    hidden_files: Option<HiddenFiles>,
    folder_sizes: Option<FolderSizes>,
    thumbnails_shown: ThumbnailsShown,
    listed_page: Option<PageRef>,
    shown_page: Option<(TabId, PageRef)>,
    known: KnownFolders,
    pub(crate) menu: Option<Menu>,
    pub(crate) shortcuts: Option<Shortcuts>,
    pub(crate) search: search::Over,
    pub(crate) focus: FocusHandle,
    pub(crate) island: Island,
    jobs: Jobs,
    pub(crate) counted: SharedString,
    showing: Option<showing::Showing>,
    tabs: Tabs,
    drop_asks: aiming::Asks,
    hover_open: hover_open::HoverOpen,
    tab_order: tab_order::TabOrder,
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

    const FIRST_ASKS: [Command; 2] = [Command::Ping, Command::WindowState];

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
        crate::keep_opener(connector, cx);
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
            status,
            connection,
            listing: None,
            details: HashMap::new(),
            shares: shares::Shares::default(),
            theme_icons: ThemeIcons::default(),
            rows_wanted: 0..0,
            rows_drawn: 0..0,
            sizing: views::Sizing::default(),
            selection: None,
            jump: TypeJump::default(),
            went: Went::Stayed,
            band: None,
            pressed_name: false,
            lift: None,
            edge: EdgeScroll::default(),
            bar: ScrollBar::default(),
            grip: None,
            selected_on_press: None,
            selection_before_press: None,
            pressed_text: None,
            slow_rename: None,
            scale_factor: DEFAULT_SCALE_FACTOR,
            asked: None,
            requested: None,
            order: Order::default(),
            asked_order: Order::default(),
            shown_order: Order::default(),
            core_moved: false,
            next_listing: 0,
            fallbacks: vec![PathBuf::from(ROOT_FOLDER), home.clone()],
            falling_back: false,
            home,
            history: History::default(),
            path_bar: cx.new(|_| PathBar::new(&host)),
            asks_sent: 0,
            scroll: UniformListScrollHandle::new(),
            bounds_watch: None,
            hidden_files: None,
            folder_sizes: None,
            thumbnails_shown: ThumbnailsShown::Shown,
            listed_page: None,
            shown_page: None,
            known: KnownFolders::default(),
            menu: None,
            shortcuts: None,
            search: search::Over::default(),
            focus: host.list.clone(),
            island: Island::default(),
            jobs: Jobs::default(),
            counted: SharedString::default(),
            showing: None,
            tabs: Tabs::default(),
            drop_asks: aiming::Asks::default(),
            hover_open: hover_open::HoverOpen::default(),
            tab_order: tab_order::TabOrder::default(),
            host,
            parts,
            watching: Vec::new(),
            _events: events,
        };
        window.watching = window.watch_slots(cx);
        window.go_first(location, cx);
        window
    }

    fn go_first(&mut self, location: &Location, cx: &mut Context<Self>) {
        match location {
            Location::Page(page) if page.is_start() => self.open_start(cx),
            Location::Folder(folder) => self.open(folder),
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
        self.parts.peek(&envelope, cx);
        let envelope = match envelope.open::<Event>() {
            Ok(event) => return self.on_event(event, cx),
            Err(envelope) => envelope,
        };
        self.hand_to_parts(envelope, cx);
    }

    fn hand_to_parts(&mut self, envelope: Envelope, cx: &mut Context<Self>) {
        if self.tell_left(cx) {
            let this = cx.entity().downgrade();
            cx.defer(move |cx| {
                this.update(cx, |this, cx| this.hand_to_parts(envelope, cx))
                    .ok();
            });
            return;
        }
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
        self.location().is_some_and(Location::is_start)
    }

    pub fn folder(&self) -> Option<&Path> {
        if let Some(from) = self.search.from() {
            return from.folder();
        }
        if self.listed_page.is_some() {
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

    pub const fn known_folders(&self) -> &KnownFolders {
        &self.known
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
