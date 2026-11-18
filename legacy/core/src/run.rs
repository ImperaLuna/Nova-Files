use std::collections::HashMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use nova_files_api::{
    ClientId, Command, CoreMessage, DriveOffer, Envelope, Event, EventSink, MessageType,
};
use nova_files_folders_api as folders_api;
use nova_files_folders_core::Store as FolderViewsStore;
use nova_files_messages::{Routed, Routes};
use nova_files_trash_api as trash_api;

use crate::disk_space;
use crate::drives::Store as DrivesStore;
use crate::file_icons::FileIcons;
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
use crate::trash;
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

pub fn routes() -> Result<Routes<Setup, Client>, MessageType> {
    let mut routes = Routes::new();
    routes.on(|setup: &Setup, id, client: &Client, command: Command| {
        answer(id, client, command, setup)
    })?;
    routes.on(|setup: &Setup, _, client: &Client, command| match command {
        trash_api::Command::List => {
            nova_files_trash_core::list(trash::trashes(setup.home.as_ref()), client.events())
        }
    })?;
    routes.on(|setup: &Setup, id, client: &Client, command| {
        match command {
            folders_api::Command::Views => setup.folder_views.listen(id, client.events()),
            folders_api::Command::SetView { folder, view } => setup.folder_views.set(folder, view),
        }
        Ok(())
    })?;
    routes.on_leave(forget);
    Ok(routes)
}

pub fn routed_types() -> Result<Vec<MessageType>, MessageType> {
    Ok(routes()?.takes().collect())
}

pub fn emitted_types() -> Vec<MessageType> {
    vec![
        MessageType::of::<Event>(),
        MessageType::of::<trash_api::Event>(),
        MessageType::of::<folders_api::Event>(),
    ]
}

pub fn run(receiver: &Receiver<CoreMessage>, idle_exit: Duration, setup: &Setup) {
    let routes = match routes() {
        Ok(routes) => routes,
        Err(twice) => {
            log::error!("two routes for {}", twice.name());
            return;
        }
    };
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
        handle(&mut clients, message, &routes, setup);
    }
}

fn handle(
    clients: &mut HashMap<ClientId, Client>,
    message: CoreMessage,
    routes: &Routes<Setup, Client>,
    setup: &Setup,
) {
    match message {
        CoreMessage::Connected { client, events } => {
            let events: Arc<dyn EventSink> = Arc::from(events);
            let started = Client::new(events, setup.thumbnails.as_ref(), &setup.file_icons);
            clients.insert(client, started);
        }
        CoreMessage::Disconnected { client } => {
            clients.remove(&client);
            routes.leave(setup, client);
        }
        CoreMessage::Activity => {}
        CoreMessage::Offer(offer) => take_offer(offer, setup),
        CoreMessage::Command { client, command } => {
            let Some(target) = clients.get(&client) else {
                return;
            };
            match routes.route(setup, client, target, command) {
                Ok(Routed::Handled) => {}
                Ok(Routed::Unrouted(kind)) => log::error!("no route for {}", kind.name()),
                Err(_) => {
                    clients.remove(&client);
                    routes.leave(setup, client);
                }
            }
        }
    }
}

fn take_offer(offer: Envelope, setup: &Setup) {
    match offer.open::<DriveOffer>() {
        Ok(DriveOffer(service)) => setup.drives.offer(service),
        Err(other) => log::error!("no use for an offer of {}", other.kind().name()),
    }
}

fn forget(setup: &Setup, client: ClientId) {
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
        | Command::Rows { .. }
        | Command::Search { .. }
        | Command::Find { .. }
        | Command::Properties { .. }
        | Command::Preview { .. }
        | Command::OpenFile { .. }
        | Command::OpenWith(_)
        | Command::FolderUsed(_)
        | Command::RecentFolders { .. }) => {
            folders::answer(id, client, command, setup);
            Ok(())
        }
        command @ (Command::WindowState
        | Command::SaveWindowState(_)
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
