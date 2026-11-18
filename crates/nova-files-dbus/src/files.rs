use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use futures::channel::oneshot;
use nova_files_api::{Command, Connector, Event};
use zbus::fdo;

pub const INTERFACE: &str = "io.github.ImperaLuna.NovaFiles1";

const CORE_ANSWER_TIMEOUT: Duration = Duration::from_secs(2);

const WAIT_THREAD: &str = "nova-files-dbus-call";

pub struct Files {
    connector: Connector,
}

impl Files {
    pub const fn new(connector: Connector) -> Self {
        Self { connector }
    }
}

pub async fn ask_core(connector: &Connector, command: Command) -> fdo::Result<Event> {
    let connector = connector.clone();
    let (answer_sender, answer) = oneshot::channel();
    // zbus answers every call on one executor, so a blocking wait there holds up every other call.
    // https://docs.rs/zbus/latest/zbus/struct.Connection.html
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
    async fn ping(&self) -> fdo::Result<()> {
        match ask_core(&self.connector, Command::Ping).await? {
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
