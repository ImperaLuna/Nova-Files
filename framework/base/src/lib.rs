//! The commands, events and types the core and its clients share. Plain std
//! types, no I/O.
//!
//! A client (a window, the D-Bus service) gets a [`Connection`] from a
//! [`Connector`], sends [`Command`]s through it and receives [`Event`]s
//! through the [`EventSink`] it handed over. A client that asks for the
//! [`Theme`], the [`DriveList`] or the places gets them
//! again whenever they change. The core reads the other end of the channel
//! as [`CoreMessage`]s, and watches drives through a [`DriveService`]. File
//! operations run as jobs (see [`Operation`]). A [`Size`] is what a scan of
//! files and folders counted. A [`DiskSpace`] is how full a drive is. A
//! [`RecentFolder`] is one a person opened lately, for the Start page.
//! [`Gathered`] folders make up the Games and Projects places. A
//! [`FolderView`] is how a folder is shown. A [`FileIcon`] is how an
//! entry's icon is drawn, and a [`ThumbnailSize`] how big its thumbnail is.
//! A [`WindowRequest`] is a window another program asks for.

#![forbid(unsafe_code)]

mod access;
mod bookmark;
mod disk_space;
mod drive;
mod file_icon;
mod file_type;
mod folder;
mod folder_view;
mod gathered;
mod hidden_files;
mod job;
mod message;
mod open_with;
mod preview;
mod properties;
mod recent;
mod search;
mod size;
mod theme;
mod thumbnail_size;
pub mod uri;
mod window;

pub use access::{AccessChange, MODE_BITS, ModeChange, Reach};
pub use bookmark::{Bookmark, BookmarkChange};
pub use disk_space::{DiskSpace, MAX_DISK_SPACE_PATHS, PathSpace};
pub use drive::{
    Drive, DriveAction, DriveFailure, DriveId, DriveList, DriveOffer, DriveService, Volume,
    VolumeId,
};
pub use file_icon::{FILE_ICON_PIXELS, FileIcon, IconFormat, IconKind, ThemeIcon};
pub use file_type::{FileTypes, glob_matches};
pub use folder::{
    Details, Entry, EntryId, EntryKind, Listing, ListingId, Order, Place, PlaceKind, SortKey,
    Unreadable,
};
pub use folder_view::{FolderView, FolderViews, IconSize, MAX_FOLDER_VIEWS, ViewMode};
pub use gathered::{Gathered, GatheredChange, GatheredFolder, Gathering, Holds};
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
pub use preview::{Fit, MAX_FIT_SIDE, Picture};
pub use properties::{Account, PathKind, Properties, PropertiesId};
pub use recent::{MAX_RECENT_FOLDERS, PEEK_ENTRIES, Peeked, RecentFolder, RecentId};
pub use search::{Hit, Matched, SearchFailed, SearchId};
pub use size::{MeasureId, ScanState, Share, Size};
pub use theme::{Color, DesktopLook, Mode, Palette, PaletteKey, SWATCH_COUNT, Theme};
pub use thumbnail_size::ThumbnailSize;
pub use window::WindowRequest;

/// The application id: the D-Bus name, the Wayland app id and the name of
/// the D-Bus service file. Set once, in `.cargo/config.toml`.
pub const APP_ID: &str = env!("NOVA_FILES_APP_ID");
