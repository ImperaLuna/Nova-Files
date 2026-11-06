//! The Nova Files interface: what launchers call. `Ping` answers once the
//! core has answered, so a reply means the core itself is running.
//!
//! Each call opens its own connection to the core and drops it after the
//! answer, so D-Bus traffic counts as activity without keeping the core
//! awake on its own.

use std::sync::mpsc;

use nova_files_api::{Command, Connector, Event};
use zbus::fdo;

use crate::CALL_TIMEOUT;

pub struct Files {
    connector: Connector,
}

impl Files {
    pub const fn new(connector: Connector) -> Self {
        Self { connector }
    }

    /// Sends `command` to the core and waits up to [`CALL_TIMEOUT`] for its
    /// answer.
    fn ask_core(&self, command: Command) -> fdo::Result<Event> {
        let (events, answers) = mpsc::channel();
        let connection = self
            .connector
            .connect(events)
            .map_err(|error| fdo::Error::Failed(error.to_string()))?;
        connection
            .send(command)
            .map_err(|error| fdo::Error::Failed(error.to_string()))?;
        answers
            .recv_timeout(CALL_TIMEOUT)
            .map_err(|_| fdo::Error::Timeout("the core did not answer".to_owned()))
    }
}

// The interface name is versioned on its own; it is not the application id.
#[zbus::interface(name = "io.github.ImperaLuna.NovaFiles1")]
impl Files {
    /// Returns once the core has answered.
    fn ping(&self) -> fdo::Result<()> {
        match self.ask_core(Command::Ping)? {
            Event::Pong => Ok(()),
        }
    }
}
