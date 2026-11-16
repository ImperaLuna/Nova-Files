//! The core's loop: read one message at a time and answer it.
//!
//! While any client is connected or any job runs or is paused, the loop
//! waits without a deadline. Otherwise it waits at most the idle time; any
//! message (a D-Bus ping, a window about to open, a job ending) starts the
//! wait again, and silence ends the loop.
//!
//! Commands that touch the disk run as jobs on threads of their own (see
//! `jobs`, and `ops` for file operations), so a slow disk never holds up
//! the loop or other clients.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use nova_files_api::{ClientId, Command, CoreMessage, Event, EventSink};

use crate::disk_space;
use crate::drives::Store as DrivesStore;
use crate::jobs::Client;
use crate::measure::Measurer;
use crate::mime::MimeTypes;
use crate::open::OpenWithWorker;
use crate::ops::Jobs;
use crate::places::{Home, Store as PlacesStore};
use crate::recent::Store as RecentStore;
use crate::search::Searcher;
use crate::settings::Store as SettingsStore;
use crate::theme::Store as ThemeStore;
use crate::thumbs::Thumbnails;
use crate::window_state::Store as WindowStateStore;

mod folders;
mod look;
mod operations;

/// Where the core finds and keeps things.
pub struct Setup {
    /// The user's folders, for the trash listing.
    pub home: Option<Home>,
    /// The places list and the pinned folders.
    pub places: PlacesStore,
    pub window_state: WindowStateStore,
    pub settings: SettingsStore,
    pub theme: ThemeStore,
    pub thumbnails: Option<Thumbnails>,
    pub searcher: Searcher,
    /// The MIME database, read at the first [`Command::OpenFile`],
    /// [`Command::OpenWith`] or [`Command::Preview`] and kept while the
    /// core runs.
    pub mime_types: Arc<OnceLock<MimeTypes>>,
    /// Answers Open With requests, in order.
    pub open_with: OpenWithWorker,
    /// File operations, and the clients that hear about them.
    pub jobs: Arc<Jobs>,
    /// Size scans, each for the client that asked.
    pub measurer: Measurer,
    /// The drives, and the clients that hear about them.
    pub drives: DrivesStore,
    /// The folders a person opened lately.
    pub recent: RecentStore,
}

/// Runs until idle.
pub fn run(receiver: &Receiver<CoreMessage>, idle_exit: Duration, setup: &Setup) {
    let mut clients: HashMap<ClientId, Client> = HashMap::new();
    loop {
        let message = if clients.is_empty() && !setup.jobs.busy() {
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
            forget(client, setup);
        }
        // Receiving it already restarted the idle wait.
        CoreMessage::Activity => {}
        CoreMessage::DriveService(service) => setup.drives.offer(service),
        CoreMessage::Command { client, command } => {
            let Some(target) = clients.get(&client) else {
                return;
            };
            if answer(client, target, command, setup).is_err() {
                clients.remove(&client);
                forget(client, setup);
            }
        }
    }
}

/// Stops telling `client` about anything it asked to hear about.
fn forget(client: ClientId, setup: &Setup) {
    setup.settings.forget(client);
    setup.theme.forget(client);
    setup.places.forget(client);
    setup.drives.forget(client);
    setup.jobs.audience().leave(client);
    setup.measurer.forget(client);
}

/// Answers `command`, at once or through a job. The folder, look and job
/// commands are answered in a file each (see `folders`, `look` and
/// `operations`); the few others here. This match is exhaustive, so a new
/// command doesn't compile until it is sent somewhere; the area it goes to
/// needs its arm too, as each area's own match passes over the rest.
///
/// # Errors
/// [`nova_files_api::ClientGone`] when the client stopped listening.
fn answer(
    id: ClientId,
    client: &Client,
    command: Command,
    setup: &Setup,
) -> Result<(), nova_files_api::ClientGone> {
    match command {
        Command::Ping => client.deliver(Event::Pong),
        command @ (Command::ListFolder { .. }
        | Command::Places
        | Command::ChangeBookmark(_)
        | Command::ListTrash
        | Command::Rows { .. }
        | Command::Search { .. }
        | Command::Find { .. }
        | Command::Properties { .. }
        | Command::Preview { .. }
        | Command::OpenFile { .. }
        | Command::OpenWith(_)
        | Command::FolderUsed(_)
        | Command::RecentFolders { .. }) => folders::answer(id, client, command, setup),
        command @ (Command::WindowState
        | Command::SaveWindowState(_)
        | Command::Settings
        | Command::ChangeSetting(_)
        | Command::Theme
        | Command::DesktopLook(_)) => look::answer(id, client, command, setup),
        command @ (Command::WatchJobs
        | Command::StartJob { .. }
        | Command::DropFiles { .. }
        | Command::DropWould { .. }
        | Command::PauseJob(_)
        | Command::ResumeJob(_)
        | Command::ForgetJob(_)
        | Command::CancelJob(_)
        | Command::AnswerConflict { .. }) => operations::answer(id, client, command, setup),
        Command::Drives => setup.drives.listen(id, client.events()),
        Command::Drive(action) => setup.drives.act(client.events(), action),
        Command::DiskSpace(paths) => {
            disk_space::answer(client.events(), paths);
            Ok(())
        }
        command @ (Command::Measure { .. } | Command::Space { .. } | Command::StopMeasure(_)) => {
            setup.measurer.answer(id, client.events(), command);
            Ok(())
        }
    }
}
