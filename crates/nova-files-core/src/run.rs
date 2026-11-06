//! The core's loop: read one message at a time and answer it.
//!
//! While any client is connected the loop waits without a deadline. With
//! none, it waits at most the idle time; any message (a D-Bus ping, a
//! window about to open) starts the wait again, and silence ends the loop.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use nova_files_api::{ClientId, Command, CoreMessage, Event, EventSink};

pub fn run(receiver: &Receiver<CoreMessage>, idle_exit: Duration) {
    let mut clients: HashMap<ClientId, Box<dyn EventSink>> = HashMap::new();
    loop {
        let message = if clients.is_empty() {
            match receiver.recv_timeout(idle_exit) {
                Ok(message) => message,
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match receiver.recv() {
                Ok(message) => message,
                Err(_) => return,
            }
        };
        handle(&mut clients, message);
    }
}

fn handle(clients: &mut HashMap<ClientId, Box<dyn EventSink>>, message: CoreMessage) {
    match message {
        CoreMessage::Connected { client, events } => {
            clients.insert(client, events);
        }
        CoreMessage::Disconnected { client } => {
            clients.remove(&client);
        }
        // Receiving it already restarted the idle wait.
        CoreMessage::Activity => {}
        CoreMessage::Command { client, command } => {
            let event = answer(&command);
            let delivered = clients.get(&client).map(|events| events.deliver(event));
            if matches!(delivered, Some(Err(_))) {
                clients.remove(&client);
            }
        }
    }
}

const fn answer(command: &Command) -> Event {
    match command {
        Command::Ping => Event::Pong,
    }
}
