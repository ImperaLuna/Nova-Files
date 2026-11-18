use std::collections::HashMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use nova_files_api::{ClientId, Command, CoreMessage, Event, EventSink};

use crate::disk_space;
use crate::drives::Store as DrivesStore;
use crate::file_icons::FileIcons;
use crate::folder_views::Store as FolderViewsStore;
use crate::gathered::Store as GatheredStore;
use crate::inside::Inside;
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

pub struct Setup {
    pub home: Option<Home>,
    pub places: PlacesStore,
    pub gathered: GatheredStore,
    pub window_state: WindowStateStore,
    pub settings: SettingsStore,
    pub theme: ThemeStore,
    pub thumbnails: Option<Thumbnails>,
    pub searcher: Searcher,
    pub mime_types: Arc<OnceLock<MimeTypes>>,
    pub file_icons: Arc<FileIcons>,
    pub open_with: OpenWithWorker,
    pub jobs: Arc<Jobs>,
    pub measurer: Measurer,
    pub drives: DrivesStore,
    pub recent: RecentStore,
    pub folder_views: FolderViewsStore,
    pub inside: Arc<Inside>,
}

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
            let started = Client::new(events, setup.thumbnails.as_ref(), &setup.file_icons);
            clients.insert(client, started);
        }
        CoreMessage::Disconnected { client } => {
            clients.remove(&client);
            forget(client, setup);
        }
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

fn forget(client: ClientId, setup: &Setup) {
    setup.settings.forget(client);
    setup.theme.forget(client);
    setup.places.forget(client);
    setup.gathered.forget(client);
    setup.folder_views.forget(client);
    setup.drives.forget(client);
    setup.jobs.audience().leave(client);
    setup.measurer.forget(client);
}

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
        | Command::Gathered
        | Command::ChangeGathered(_)
        | Command::ListGathered { .. }
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
        | Command::FolderViews
        | Command::SetFolderView { .. }
        | Command::Settings
        | Command::ChangeSetting(_)
        | Command::Theme
        | Command::DesktopLook(_)
        | Command::DesktopIconThemes(_)) => look::answer(id, client, command, setup),
        command @ (Command::WatchJobs
        | Command::StartJob { .. }
        | Command::DropFiles { .. }
        | Command::DropWould { .. }
        | Command::PauseJob(_)
        | Command::ResumeJob(_)
        | Command::ForgetJob(_)
        | Command::CancelJob(_)
        | Command::AnswerConflict { .. }
        | Command::GivePassword { .. }) => operations::answer(id, client, command, setup),
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
