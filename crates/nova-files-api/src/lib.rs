//! The commands, events and types the core and its clients share. Plain std
//! types, no I/O.
//!
//! A client (a window, the D-Bus service) gets a [`Connection`] from a
//! [`Connector`], sends [`Command`]s through it and receives [`Event`]s
//! through the [`EventSink`] it handed over. The core reads the other end
//! of the channel as [`CoreMessage`]s.

#![forbid(unsafe_code)]

mod connection;
mod folder;
mod message;

pub use connection::{
    ClientGone, Connection, Connector, CoreGone, CoreMessage, EventSink, channel,
};
pub use folder::{Entry, EntryId, EntryKind, Listing, ListingId, Place, PlaceKind, Unreadable};
pub use message::{ClientId, Command, Event};

/// The application id: the D-Bus name, the Wayland app id and the name of
/// the D-Bus service file. Set once, in `.cargo/config.toml`.
pub const APP_ID: &str = env!("NOVA_FILES_APP_ID");
