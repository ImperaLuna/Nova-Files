//! The Nova Files interface: what launchers call. `Ping` answers once the
//! core has answered, so a reply means the core itself is running.
//!
//! Each call opens its own connection to the core and drops it after the
//! answer, so D-Bus traffic counts as activity without keeping the core
//! awake on its own. The wait for the core happens on a thread of its own:
//! zbus answers every call on one executor, and a blocking wait there would
//! hold up every other call, such as a second launch's `Activate`.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use futures::channel::oneshot;
use nova_files_api::{Command, Connector, Event};
use zbus::fdo;

/// The interface name, versioned on its own; it is not the application id.
/// The `#[zbus::interface]` attribute needs the literal too; a test checks
/// the two match.
pub const INTERFACE: &str = "io.github.ImperaLuna.NovaFiles1";

/// How long a call waits for the core: well inside the caller's own
/// `CALL_TIMEOUT`, so the caller gets this error, not a timeout of its own.
const CORE_ANSWER_TIMEOUT: Duration = Duration::from_secs(2);

const WAIT_THREAD: &str = "nova-files-dbus-call";

pub struct Files {
    connector: Connector,
}

impl Files {
    pub const fn new(connector: Connector) -> Self {
        Self { connector }
    }

    /// Sends `command` to the core and waits up to [`CORE_ANSWER_TIMEOUT`]
    /// for its answer, without blocking the D-Bus executor.
    async fn ask_core(&self, command: Command) -> fdo::Result<Event> {
        let connector = self.connector.clone();
        let (answer_sender, answer) = oneshot::channel();
        thread::Builder::new()
            .name(WAIT_THREAD.to_owned())
            .spawn(move || {
                answer_sender.send(wait_for_core(&connector, command)).ok();
            })
            .map_err(|error| fdo::Error::Failed(error.to_string()))?;
        answer
            .await
            .map_err(|_| fdo::Error::Failed("the core stopped".to_owned()))?
    }
}

fn wait_for_core(connector: &Connector, command: Command) -> fdo::Result<Event> {
    let core_gone = |_| fdo::Error::Failed("the core has stopped".to_owned());
    let (events, answers) = mpsc::channel();
    let connection = connector.connect(events).map_err(core_gone)?;
    connection.send(command).map_err(core_gone)?;
    answers
        .recv_timeout(CORE_ANSWER_TIMEOUT)
        .map_err(|error| match error {
            RecvTimeoutError::Timeout => fdo::Error::Timeout("the core did not answer".to_owned()),
            RecvTimeoutError::Disconnected => fdo::Error::Failed("the core has stopped".to_owned()),
        })
}

#[zbus::interface(name = "io.github.ImperaLuna.NovaFiles1")]
impl Files {
    /// Returns once the core has answered.
    async fn ping(&self) -> fdo::Result<()> {
        match self.ask_core(Command::Ping).await? {
            Event::Pong => Ok(()),
            _ => Err(fdo::Error::Failed(
                "the core gave an unexpected answer".to_owned(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use zbus::object_server::Interface as _;

    use super::{Files, INTERFACE};

    #[test]
    fn interface_name_matches_the_constant() {
        assert_eq!(Files::name().as_str(), INTERFACE);
    }
}
