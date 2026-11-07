//! The channel between the core and its clients.
//!
//! The core owns the receiving end ([`channel`] returns it). Clients share a
//! [`Connector`] and open one [`Connection`] each. A connection tells the
//! core when it opens and when it is dropped, which is how the core knows
//! it is in use. Events go back through the client's own [`EventSink`], so
//! each client picks how it wants to be woken (a std channel, an async
//! channel) and the core never needs to know.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};

use crate::message::{ClientId, Command, Event};

/// Where the core delivers one client's events. Shared between threads:
/// the core answers some commands from worker threads.
pub trait EventSink: Send + Sync + 'static {
    /// Hands `event` to the client.
    ///
    /// # Errors
    /// [`ClientGone`] when the client stopped listening; the core then
    /// forgets it.
    fn deliver(&self, event: Event) -> Result<(), ClientGone>;
}

impl EventSink for Sender<Event> {
    fn deliver(&self, event: Event) -> Result<(), ClientGone> {
        self.send(event).map_err(|_| ClientGone)
    }
}

/// The client stopped listening; returned by [`EventSink::deliver`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientGone;

/// The core has stopped, so nothing can be sent to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreGone;

impl std::fmt::Display for CoreGone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the core has stopped")
    }
}

impl std::error::Error for CoreGone {}

/// What the core receives: commands, framed by each client's arrival and
/// departure.
pub enum CoreMessage {
    /// A client opened a connection; its events go to `events`.
    Connected {
        client: ClientId,
        events: Box<dyn EventSink>,
    },
    /// A client sent a command.
    Command { client: ClientId, command: Command },
    /// A client dropped its connection.
    Disconnected { client: ClientId },
    /// Something is about to use the core (a window is being opened), so
    /// its idle wait starts again.
    Activity,
}

impl std::fmt::Debug for CoreMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connected { client, .. } => f
                .debug_struct("Connected")
                .field("client", client)
                .finish_non_exhaustive(),
            Self::Command { client, command } => f
                .debug_struct("Command")
                .field("client", client)
                .field("command", command)
                .finish(),
            Self::Disconnected { client } => f
                .debug_struct("Disconnected")
                .field("client", client)
                .finish(),
            Self::Activity => f.write_str("Activity"),
        }
    }
}

/// Creates the channel: the [`Connector`] for clients and the receiver the
/// core reads.
pub fn channel() -> (Connector, Receiver<CoreMessage>) {
    let (sender, receiver) = mpsc::channel();
    let connector = Connector {
        sender,
        next_client: Arc::new(AtomicU64::new(0)),
    };
    (connector, receiver)
}

/// Opens connections to the core. Cheap to clone and safe to share between
/// threads.
#[derive(Clone)]
pub struct Connector {
    sender: Sender<CoreMessage>,
    next_client: Arc<AtomicU64>,
}

impl Connector {
    /// Tells the core it is about to be used, so it doesn't stop while a
    /// client is still starting up.
    ///
    /// # Errors
    /// [`CoreGone`] when the core has stopped.
    pub fn activity(&self) -> Result<(), CoreGone> {
        self.sender
            .send(CoreMessage::Activity)
            .map_err(|_| CoreGone)
    }

    /// Opens a connection whose events go to `events`.
    ///
    /// # Errors
    /// [`CoreGone`] when the core has stopped.
    pub fn connect(&self, events: impl EventSink) -> Result<Connection, CoreGone> {
        let client = ClientId(self.next_client.fetch_add(1, Ordering::Relaxed));
        self.sender
            .send(CoreMessage::Connected {
                client,
                events: Box::new(events),
            })
            .map_err(|_| CoreGone)?;
        Ok(Connection {
            client,
            sender: self.sender.clone(),
        })
    }
}

/// One client's line to the core. Dropping it tells the core the client
/// has left.
pub struct Connection {
    client: ClientId,
    sender: Sender<CoreMessage>,
}

impl Connection {
    /// This connection's id, as the core sees it.
    pub const fn client(&self) -> ClientId {
        self.client
    }

    /// Sends `command` to the core. Its answer arrives through this
    /// connection's [`EventSink`].
    ///
    /// # Errors
    /// [`CoreGone`] when the core has stopped.
    pub fn send(&self, command: Command) -> Result<(), CoreGone> {
        self.sender
            .send(CoreMessage::Command {
                client: self.client,
                command,
            })
            .map_err(|_| CoreGone)
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // The core may already have stopped; then there is no one to tell.
        let _ = self.sender.send(CoreMessage::Disconnected {
            client: self.client,
        });
    }
}
