//! The commands, events and types the core and its clients share. Plain std
//! types, no I/O.
//!
//! A client (a window, the D-Bus service) gets a [`Connection`] from a
//! [`Connector`], sends [`Command`]s through it and receives [`Event`]s
//! through the [`EventSink`] it handed over. The core reads the other end
//! of the channel as [`CoreMessage`]s. A [`PickRequest`] carries another
//! app's file picker request from D-Bus to the UI.

#![forbid(unsafe_code)]

mod connection;
mod file_type;
mod folder;
mod message;
mod pick;
pub mod uri;

pub use connection::{
    ClientGone, Connection, Connector, CoreGone, CoreMessage, EventSink, channel,
};
pub use file_type::{FileTypes, glob_matches};
pub use folder::{
    Details, Entry, EntryId, EntryKind, Listing, ListingId, Place, PlaceKind, Unreadable,
};
pub use message::{ClientId, Command, Event, MAX_ROWS};
pub use pick::{FileFilter, FilterRule, PickAnswer, PickId, PickKind, PickReply, PickRequest};

/// The application id: the D-Bus name, the Wayland app id and the name of
/// the D-Bus service file. Set once, in `.cargo/config.toml`.
pub const APP_ID: &str = env!("NOVA_FILES_APP_ID");
