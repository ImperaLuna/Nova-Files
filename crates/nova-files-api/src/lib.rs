//! The commands, events and types the core and its clients share. Plain std
//! types, no I/O.
//!
//! A client (a window, the D-Bus service) gets a [`Connection`] from a
//! [`Connector`], sends [`Command`]s through it and receives [`Event`]s
//! through the [`EventSink`] it handed over. A client that asks for the
//! [`Settings`] or the [`Theme`] gets them again whenever they change. The
//! core reads the other end of the channel as [`CoreMessage`]s. File
//! operations run as jobs (see [`Operation`]). A [`PickRequest`] carries
//! another app's file picker request from D-Bus to the UI.

#![forbid(unsafe_code)]

mod connection;
mod file_type;
mod folder;
mod job;
mod message;
mod pick;
mod search;
mod settings;
mod theme;
pub mod uri;

pub use connection::{
    ClientGone, Connection, Connector, CoreGone, CoreMessage, EventSink, channel,
};
pub use file_type::{FileTypes, glob_matches};
pub use folder::{
    Details, Entry, EntryId, EntryKind, Listing, ListingId, Place, PlaceKind, Unreadable,
};
pub use job::{
    Answer, Conflict, ConflictKind, Failure, JobId, JobTag, Operation, Outcome, Progress, Scope,
};
pub use message::{ClientId, Command, Event, MAX_HITS, MAX_ROWS};
pub use pick::{
    FileFilter, FilterRule, ParentWindow, PickAnswer, PickId, PickKind, PickReply, PickRequest,
};
pub use search::{Hit, Matched, SearchFailed, SearchId};
pub use settings::{Appearance, SettingChange, Settings, Thumbnails};
pub use theme::{Color, Mode, Palette, PaletteKey, SWATCH_COUNT, Theme};

/// The application id: the D-Bus name, the Wayland app id and the name of
/// the D-Bus service file. Set once, in `.cargo/config.toml`.
pub const APP_ID: &str = env!("NOVA_FILES_APP_ID");
