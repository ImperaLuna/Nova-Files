//! The core's loop: read one message at a time and answer it.
//!
//! While any client is connected the loop waits without a deadline. With
//! none, it waits at most the idle time; any message (a D-Bus ping, a
//! window about to open) starts the wait again, and silence ends the loop.
//!
//! Commands that touch the disk run as jobs on threads of their own (see
//! `jobs`), so a slow disk never holds up the loop or other clients.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use nova_files_api::{ClientId, Command, CoreMessage, Event, EventSink};

use crate::jobs::{self, Client};
use crate::mime::MimeTypes;
use crate::places::Home;
use crate::search::Searcher;
use crate::thumbs::Thumbnails;

/// Where the core finds and keeps things.
pub struct Setup {
    pub home: Option<Home>,
    pub thumbnails: Option<Thumbnails>,
    pub searcher: Searcher,
    /// The MIME database, read at the first [`Command::OpenFile`] and kept
    /// while the core runs.
    pub mime_types: Arc<OnceLock<MimeTypes>>,
}

/// Runs until idle.
pub fn run(receiver: &Receiver<CoreMessage>, idle_exit: Duration, setup: &Setup) {
    let mut clients: HashMap<ClientId, Client> = HashMap::new();
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
        handle(&mut clients, message, setup);
    }
}

fn handle(clients: &mut HashMap<ClientId, Client>, message: CoreMessage, setup: &Setup) {
    match message {
        CoreMessage::Connected { client, events } => {
            let events: Arc<dyn EventSink> = Arc::from(events);
            clients.insert(client, Client::new(events, setup.thumbnails.as_ref()));
        }
        CoreMessage::Disconnected { client } => {
            // Dropping the client stops its listing.
            clients.remove(&client);
        }
        // Receiving it already restarted the idle wait.
        CoreMessage::Activity => {}
        CoreMessage::Command { client, command } => {
            let Some(target) = clients.get(&client) else {
                return;
            };
            if answer(target, command, setup).is_err() {
                clients.remove(&client);
            }
        }
    }
}

/// Answers `command`, at once or through a job.
///
/// # Errors
/// [`nova_files_api::ClientGone`] when the client stopped listening.
fn answer(
    client: &Client,
    command: Command,
    setup: &Setup,
) -> Result<(), nova_files_api::ClientGone> {
    match command {
        Command::Ping => client.deliver(Event::Pong),
        Command::ListFolder { id, path } => {
            jobs::list_folder(client, id, path);
            Ok(())
        }
        Command::Places => {
            jobs::places(client, setup.home.clone());
            Ok(())
        }
        Command::Rows { id, first, count } => {
            client.want_rows(id, first, count);
            Ok(())
        }
        Command::Search { id, query, limit } => {
            setup.searcher.search(client, id, query, limit);
            Ok(())
        }
        Command::OpenFile { path, activation } => {
            jobs::open_file(client, Arc::clone(&setup.mime_types), path, activation);
            Ok(())
        }
    }
}
