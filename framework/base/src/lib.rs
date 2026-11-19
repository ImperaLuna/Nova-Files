//! The commands, events and types the core and its clients share. Plain std
//! types, no I/O.
//!
//! A client (a window, the D-Bus service) gets a [`Connection`] from a
//! [`Connector`], sends [`Command`]s through it and receives [`Event`]s
//! through the [`EventSink`] it handed over. The core reads the other end
//! of the channel as [`CoreMessage`]s. File operations run as jobs (see
//! [`Operation`]). A [`Preview`] is what a quick look shows of a file or
//! folder. A [`FolderView`] is how a folder is shown. A [`FileIcon`] is how
//! an entry's icon is drawn, and a [`ThumbnailSize`] how big its thumbnail is.
//! A [`WindowRequest`] is a window another program asks for.

#![forbid(unsafe_code)]

mod access;
mod file_icon;
mod file_type;
mod folder;
mod folder_view;
mod hidden_files;
mod job;
mod message;
mod open_with;
mod preview;
mod search;
mod theme;
mod thumbnail_size;
pub mod uri;
mod window;

pub use access::{AccessChange, MODE_BITS, ModeChange, Reach};
pub use file_icon::{FILE_ICON_PIXELS, FileIcon, IconFormat, IconKind, ThemeIcon};
pub use file_type::{FileTypes, glob_matches};
pub use folder::{
    Details, Entry, EntryId, EntryKind, Listing, ListingId, Order, SortKey, Unreadable,
};
pub use folder_view::{FolderView, FolderViews, IconSize, MAX_FOLDER_VIEWS, ViewMode};
pub use hidden_files::HiddenFiles;
pub use job::{
    Answer, Conflict, ConflictKind, DropAskId, DropKind, ExtractTo, Failure, JobId, JobTag,
    Operation, Outcome, Packing, Progress, Scope, Tally,
};
pub use message::{Command, Event, MAX_FOUND, MAX_HITS, MAX_ROWS};
pub use nova_files_messages::{
    ClientGone, ClientId, Connection, Connector, CoreGone, CoreMessage, Envelope, EventSink,
    Message, MessageType, Routed, Routes, channel, missing_routes,
};
pub use open_with::{App, NotOpened, OpenWith, OpenWithAnswer, OpenWithId};
pub use preview::{
    Fit, MAX_FIT_SIDE, PREVIEW_ENTRIES, PREVIEW_TEXT_BYTES, Picture, Preview, PreviewContent,
    PreviewId,
};
pub use search::{Hit, Matched, SearchFailed, SearchId};
pub use theme::{Color, DesktopLook, Mode, Palette, PaletteKey, SWATCH_COUNT, Theme};
pub use thumbnail_size::ThumbnailSize;
pub use window::WindowRequest;

/// The application id: the D-Bus name, the Wayland app id and the name of
/// the D-Bus service file. Set once, in `.cargo/config.toml`.
pub const APP_ID: &str = env!("NOVA_FILES_APP_ID");
