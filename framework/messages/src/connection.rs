use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};

use crate::envelope::{Envelope, Message};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClientId(pub u64);

pub trait EventSink: Send + Sync + 'static {
    fn deliver_envelope(&self, envelope: Envelope) -> Result<(), ClientGone>;
}

impl dyn EventSink {
    pub fn deliver<M: Message>(&self, message: M) -> Result<(), ClientGone> {
        self.deliver_envelope(Envelope::new(message))
    }
}

impl<M: Message> EventSink for Sender<M> {
    fn deliver_envelope(&self, envelope: Envelope) -> Result<(), ClientGone> {
        let Some(message) = envelope.taken_as::<M>() else {
            return Ok(());
        };
        self.send(message).map_err(|_| ClientGone)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientGone;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreGone;

impl std::fmt::Display for CoreGone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the core has stopped")
    }
}

impl std::error::Error for CoreGone {}

pub enum CoreMessage {
    Connected {
        client: ClientId,
        events: Box<dyn EventSink>,
    },
    Command {
        client: ClientId,
        command: Envelope,
    },
    Disconnected {
        client: ClientId,
    },
    Activity,
    Offer(Envelope),
    Stop,
}

impl CoreMessage {
    pub fn into_command<M: Message>(self) -> Option<M> {
        match self {
            Self::Command { command, .. } => command.open().ok(),
            _ => None,
        }
    }

    pub fn command<M: Message>(&self) -> Option<&M> {
        match self {
            Self::Command { command, .. } => command.get(),
            _ => None,
        }
    }
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
            Self::Stop => f.write_str("Stop"),
            Self::Offer(offer) => f.debug_tuple("Offer").field(&offer.kind().name()).finish(),
        }
    }
}

pub fn channel() -> (Connector, Receiver<CoreMessage>) {
    let (sender, receiver) = mpsc::channel();
    let connector = Connector {
        sender,
        next_client: Arc::new(AtomicU64::new(0)),
    };
    (connector, receiver)
}

#[derive(Clone)]
pub struct Connector {
    sender: Sender<CoreMessage>,
    next_client: Arc<AtomicU64>,
}

impl Connector {
    pub fn stop(&self) -> Result<(), CoreGone> {
        self.sender.send(CoreMessage::Stop).map_err(|_| CoreGone)
    }

    pub fn activity(&self) -> Result<(), CoreGone> {
        self.sender
            .send(CoreMessage::Activity)
            .map_err(|_| CoreGone)
    }

    pub fn offer<M: Message>(&self, offer: M) -> Result<(), CoreGone> {
        self.sender
            .send(CoreMessage::Offer(Envelope::new(offer)))
            .map_err(|_| CoreGone)
    }

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

pub struct Connection {
    client: ClientId,
    sender: Sender<CoreMessage>,
}

impl Connection {
    pub const fn client(&self) -> ClientId {
        self.client
    }

    pub fn send<M: Message>(&self, command: M) -> Result<(), CoreGone> {
        self.send_envelope(Envelope::new(command))
    }

    pub fn send_envelope(&self, command: Envelope) -> Result<(), CoreGone> {
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
        self.sender
            .send(CoreMessage::Disconnected {
                client: self.client,
            })
            .ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    struct Hello;

    #[test]
    fn a_connection_frames_its_commands_and_says_when_it_leaves() {
        let (connector, core) = channel();
        let connection = connector.connect(mpsc::channel::<Hello>().0).unwrap();
        connection.send(Hello).unwrap();
        drop(connection);
        let got: Vec<CoreMessage> = core.try_iter().collect();
        assert!(matches!(
            got[0],
            CoreMessage::Connected {
                client: ClientId(0),
                ..
            }
        ));
        assert_eq!(got[1].command::<Hello>(), Some(&Hello));
        assert!(matches!(
            got[2],
            CoreMessage::Disconnected {
                client: ClientId(0)
            }
        ));
    }

    #[test]
    fn a_channel_sink_drops_another_type_and_stays() {
        let (sender, receiver) = mpsc::channel::<Hello>();
        let sink: Box<dyn EventSink> = Box::new(sender);
        assert_eq!(sink.deliver(3_u8), Ok(()));
        assert_eq!(sink.deliver(Hello), Ok(()));
        assert_eq!(receiver.try_recv(), Ok(Hello));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn an_offer_reaches_the_core_with_its_type() {
        let (connector, core) = channel();
        connector.offer(Arc::new(5_u32)).unwrap();
        let Ok(CoreMessage::Offer(offer)) = core.try_recv() else {
            panic!("no offer");
        };
        assert_eq!(offer.open::<Arc<u32>>().ok().as_deref(), Some(&5));
        assert!(connector.activity().is_ok());
        drop(core);
        assert_eq!(connector.activity(), Err(CoreGone));
    }
}
