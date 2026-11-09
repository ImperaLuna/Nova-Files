//! The main window: the places on the left, the open folder's entries on
//! the right, and an Up button.
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
//! leave the history alone. Paths are tidied by their text alone (`a/../b` is `b`), as a user reads
//! them, so Up goes where the path on screen says.
//!
//! The core's events arrive through an async channel the window's own task
//! reads; closing the window drops the connection, which tells the core
//! one client fewer is using it. Drawing lives in `view`; the actions a
//! key or button sends are handled in `view` too, each by one method here
//! (see `commands`).
//!
//! As the list draws, the window tells the core which rows are on screen,
//! plus one screen either side. The core answers with those rows' details
//! (read once each; the window keeps them for the listing) and their
//! thumbnails. The window keeps only the thumbnails of those rows, shrunk
//! to the size they're drawn (see `thumbnail`). A thumbnail it lets go of
//! is dropped from GPUI's image atlas too (see `shown_thumbnails`).

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use futures::StreamExt as _;
use futures::channel::mpsc::{self, UnboundedSender};
use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, RenderImage, ScrollStrategy, Task,
    UniformListScrollHandle, Window,
};
use nova_files_api::{
    ClientGone, Command, Connection, Connector, Details, EntryId, EntryKind, Event, EventSink,
    Listing, ListingId, Place,
};

use crate::columns::{self, ColumnText};
use crate::history::History;
use crate::jump::TypeJump;
use crate::pick::Picker;
use crate::selection::Selection;
use crate::shown_thumbnails::ShownThumbnails;
use crate::theme::size;
use crate::thumbnail;

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
    /// The listing asked for most recently; only its answer counts.
    pub(crate) asked: Option<ListingId>,
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
    pub(crate) scroll: UniformListScrollHandle,
    /// Another app's request, while the window is its picker (see `pick`).
    pub(crate) pick: Option<Picker>,
    /// The home folder, which `~` names in a picker's name box.
    pub(crate) home: PathBuf,
    /// Focus for the window's own keys (see `commands`).
    pub(crate) focus: FocusHandle,
    _events: Task<()>,
}

/// A thumbnail shrunk for row `index` of listing `id`, `side` device
/// pixels across.
struct Shrunk {
    id: ListingId,
    index: usize,
    side: u32,
    image: RenderImage,
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
            details: HashMap::new(),
            thumbnails: ShownThumbnails::default(),
            rows_wanted: 0..0,
            rows_drawn: 0..0,
            selection: None,
            jump: TypeJump::default(),
            scale_factor: DEFAULT_SCALE_FACTOR,
            asked: None,
            core_moved: false,
            next_listing: 0,
            // Popped from the end: home first, then the root.
            fallbacks: vec![PathBuf::from(ROOT_FOLDER), home.clone()],
            home,
            places: Vec::new(),
            history: History::default(),
            scroll: UniformListScrollHandle::new(),
            pick: None,
            focus: cx.focus_handle(),
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
        let details = self.details.get(&index).copied().unwrap_or_default();
        Some(ColumnText {
            modified: details.modified.map(columns::modified).unwrap_or_default(),
            type_name: columns::type_name(&entry.name, entry.kind),
            size: details.size.map(columns::size).unwrap_or_default(),
        })
    }

    /// The thumbnail of row `index`, once the core has sent it.
    pub fn thumbnail(&self, index: usize) -> Option<&Arc<RenderImage>> {
        self.thumbnails.get(index)
    }

    /// The list drew rows `range`. It also draws its first row alone, each
    /// frame, to measure it; asking for that row alone would swap
    /// the request back and forth every frame, and each answer would draw
    /// another frame. The rows on screen are the last range drawn, so the
    /// request waits until the frame is done.
    pub(crate) fn rows_drawn(
        &mut self,
        range: Range<usize>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        self.rows_drawn = range;
        cx.defer_in(window, |this, window, cx| {
            this.show_rows(this.rows_drawn.clone(), window, cx);
        });
    }

    /// Rows `visible` are on screen: asks for their details and thumbnails
    /// and one screen either side, and forgets thumbnails now out of range.
    fn show_rows(&mut self, visible: Range<usize>, window: &mut Window, cx: &mut App) {
        let Some(listing) = &self.listing else { return };
        let screen = visible.len();
        let wanted = visible.start.saturating_sub(screen)
            ..visible
                .end
                .saturating_add(screen)
                .min(listing.entries.len());
        if wanted == self.rows_wanted {
            return;
        }
        let (Ok(first), Ok(count)) = (u32::try_from(wanted.start), u32::try_from(wanted.len()))
        else {
            return;
        };
        let command = Command::Rows {
            id: listing.id,
            first: EntryId(first),
            count,
        };
        if let Some(connection) = &self.connection {
            connection.send(command).ok();
        }
        release(self.thumbnails.keep_rows(&wanted), Some(window), cx);
        self.rows_wanted = wanted;
    }

    /// The scroll position of the list on screen.
    pub const fn scroll(&self) -> &UniformListScrollHandle {
        &self.scroll
    }

    pub fn places(&self) -> &[Place] {
        &self.places
    }

    /// Goes to `folder`, a new place in the history: asks the core for it,
    /// and the window moves there once it answers.
    pub fn open(&mut self, folder: &Path) {
        self.history.leave();
        self.request(folder);
    }

    /// Goes back to the place before this one, if there is one.
    pub fn go_back(&mut self) {
        if let Some(place) = self.history.back() {
            self.request(&place);
        }
    }

    /// Goes forward again after Back, if there is somewhere to go.
    pub fn go_forward(&mut self) {
        if let Some(place) = self.history.forward() {
            self.request(&place);
        }
    }

    /// Where Back goes, if anywhere.
    pub fn back_target(&self) -> Option<&Path> {
        self.history.back_target()
    }

    /// Where Forward goes, if anywhere.
    pub fn forward_target(&self) -> Option<&Path> {
        self.history.forward_target()
    }

    /// Asks the core for `folder`, leaving the history as it is: the
    /// window moves there once it answers.
    fn request(&mut self, folder: &Path) {
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
                self.history.arrived(&path);
                // Read again in place: keep the scroll position.
                let same_folder = self.folder() == Some(path.as_path());
                self.selection = self.selection_for(&listing);
                self.listing = Some(listing);
                self.core_moved = false;
                self.details = HashMap::new();
                release(self.thumbnails.take_all(), None, cx);
                self.rows_wanted = 0..0;
                if !same_folder {
                    self.scroll = UniformListScrollHandle::new();
                    self.jump = TypeJump::default();
                }
                self.pick_folder_opened(cx);
                if !same_folder && let Some(cursor) = self.cursor() {
                    self.scroll.scroll_to_item(cursor, ScrollStrategy::Nearest);
                }
                cx.emit(WindowEvent::Opened(path));
                cx.notify();
            }
            Event::FolderUnreadable { id, path, .. } if self.asked == Some(id) => {
                self.asked = None;
                self.history.refused(&path);
                match &self.listing {
                    None => {
                        if let Some(fallback) = self.fallbacks.pop() {
                            self.request(&fallback);
                        }
                    }
                    Some(listing) if self.core_moved && listing.path != path => {
                        let shown = listing.path.clone();
                        self.request(&shown);
                    }
                    Some(_) => {}
                }
                self.pick_folder_refused(id, &path);
                cx.emit(WindowEvent::Refused(path));
                // Back or Forward may have nowhere left to go.
                cx.notify();
            }
            Event::DetailsRead { id, first, details } => {
                if let Some(listing) = &self.listing
                    && listing.id == id
                    && let Ok(first) = usize::try_from(first.0)
                {
                    let rows = first..listing.entries.len();
                    self.details.extend(rows.zip(details.iter().copied()));
                    cx.notify();
                }
            }
            // Checked against the listing once shrunk, when it counts.
            Event::Thumbnail { id, entry, png } => {
                if let Ok(index) = usize::try_from(entry.0)
                    && self.rows_wanted.contains(&index)
                {
                    self.shrink_thumbnail(id, index, png, cx);
                }
            }
            // An answer for a folder the window has since moved away from.
            Event::FolderListed(stale) => self.dropped_listing(stale.id),
            // The window never searches, opens files or starts jobs yet, and
            // only a picker's opening asks for the window state (see
            // `window_state`).
            Event::FolderUnreadable { .. }
            | Event::Found { .. }
            | Event::SearchFailed { .. }
            | Event::FileOpened
            | Event::FileNotOpened
            | Event::WindowState(_)
            | Event::JobStarted { .. }
            | Event::JobRunning { .. }
            | Event::JobProgress { .. }
            | Event::JobPaused(_)
            | Event::JobResumed(_)
            | Event::JobFinished { .. } => {}
            Event::Places(places) => {
                self.places = places;
                cx.emit(WindowEvent::Places);
                cx.notify();
            }
        }
    }

    /// The window dropped listing `id`, which it no longer wanted. One newer
    /// than the folder on screen moved the core's row queues to it. A
    /// refusal still to come reads the folder on screen again (see
    /// `on_event`); with nothing asked for, nothing would, so it is read
    /// again now.
    fn dropped_listing(&mut self, id: ListingId) {
        let Some(listing) = &self.listing else { return };
        if id <= listing.id {
            return;
        }
        self.core_moved = true;
        if self.asked.is_none() {
            let shown = listing.path.clone();
            self.request(&shown);
        }
    }

    /// The window is drawing at `scale_factor`. Thumbnails shrunk for
    /// another scale (the window moved to another screen) are dropped and
    /// asked for again: an empty request first, so the core sends them anew.
    pub fn set_scale_factor(&mut self, scale_factor: f32, window: &mut Window, cx: &mut App) {
        let before = self.thumbnail_side();
        self.scale_factor = scale_factor;
        if self.thumbnail_side() == before {
            return;
        }
        release(self.thumbnails.take_all(), Some(window), cx);
        if let Some(listing) = &self.listing
            && !self.rows_wanted.is_empty()
            && let Some(connection) = &self.connection
        {
            let forget = Command::Rows {
                id: listing.id,
                first: EntryId(0),
                count: 0,
            };
            connection.send(forget).ok();
        }
        self.rows_wanted = 0..0;
    }

    /// A thumbnail's side in device pixels at the current scale.
    fn thumbnail_side(&self) -> u32 {
        thumbnail::device_side(size::THUMBNAIL, self.scale_factor)
    }

    /// Shrinks row `index`'s thumbnail off the UI thread, then keeps it.
    fn shrink_thumbnail(&self, id: ListingId, index: usize, png: Arc<[u8]>, cx: &Context<Self>) {
        let side = self.thumbnail_side();
        let shrinking = cx
            .background_executor()
            .spawn(async move { thumbnail::shrink(&png, side) });
        cx.spawn(async move |this, cx| {
            let Some(image) = shrinking.await else { return };
            let made = Shrunk {
                id,
                index,
                side,
                image,
            };
            this.update(cx, |this, cx| this.thumbnail_ready(made, cx))
                .ok();
        })
        .detach();
    }

    /// Keeps a thumbnail shrunk to `side` if its row is still wanted and
    /// the window still draws at that size.
    fn thumbnail_ready(&mut self, made: Shrunk, cx: &mut Context<Self>) {
        if self.listing.as_ref().map(|listing| listing.id) == Some(made.id)
            && self.rows_wanted.contains(&made.index)
            && made.side == self.thumbnail_side()
        {
            let replaced = self.thumbnails.insert(made.index, Arc::new(made.image));
            release(replaced, None, cx);
            cx.notify();
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

impl Focusable for MainWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// Drops `images` from GPUI's image atlas. `window` is the window being
/// drawn or updated, if any: GPUI takes it out of its list meanwhile, so
/// it has to be named.
fn release(
    images: impl IntoIterator<Item = Arc<RenderImage>>,
    mut window: Option<&mut Window>,
    cx: &mut App,
) {
    for image in images {
        cx.drop_image(image, window.as_deref_mut());
    }
}

const ROOT_FOLDER: &str = "/";

/// Where a window opens unless told otherwise: the home folder, or the
/// root folder on a system without one.
pub fn home_folder() -> PathBuf {
    std::env::home_dir().unwrap_or_else(|| PathBuf::from(ROOT_FOLDER))
}

/// `path` with `.` dropped and each `..` taking off the part before it,
/// worked out from the text alone. `..` at the root stays at the root.
pub fn tidy(path: &Path) -> PathBuf {
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
