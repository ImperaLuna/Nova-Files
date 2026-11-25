use std::sync::mpsc::Receiver;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use nova_files_api::{
    ClientId, Command, CoreMessage, Envelope, Event, EventSink, FolderSizes, MessageType,
};
use nova_files_archives_api as archives_api;
use nova_files_disk::home::Home;
use nova_files_drives_api::{Command as DrivesCommand, DriveOffer, Event as DrivesEvent};
use nova_files_drives_core::{Store as DrivesStore, disk_space};
use nova_files_entries_core::FolderSizer;
use nova_files_folders_api as folders_api;
use nova_files_folders_core::Store as FolderViewsStore;
use nova_files_gathered_api as gathered_api;
use nova_files_gathered_core::Store as GatheredStore;
use nova_files_git_api as git_api;
use nova_files_git_core::Reader as GitReader;
use nova_files_look_api as look_api;
use nova_files_look_core::FileIcons;
use nova_files_look_core::theme::Store as ThemeStore;
use nova_files_messages::{Routes, Served, serve};
use nova_files_open_api::{self as open_api, TakeOut};
use nova_files_open_core::{LazyMimes, MimeTypes, OpenWithWorker};
use nova_files_places_api as places_api;
use nova_files_places_core::Store as PlacesStore;
use nova_files_preview_api as preview_api;
use nova_files_preview_core::Request as PreviewRequest;
use nova_files_properties_api as properties_api;
use nova_files_properties_core::Measurer;
use nova_files_search_api as search_api;
use nova_files_search_core::Searcher;
use nova_files_settings_api as settings_api;
use nova_files_settings_core::Store as SettingsStore;
use nova_files_start_api as start_api;
use nova_files_start_core::Store as RecentStore;
use nova_files_thumbnails_api as thumbnails_api;
use nova_files_thumbnails_core::Thumbnails;
use nova_files_transfer_api as transfer_api;
use nova_files_trash_api as trash_api;

use crate::backend::jobs::Client;
use crate::backend::ops::Jobs;
use nova_files_archives_core::inside::{Inside, within};
use nova_files_disk::window_state::Store as WindowStateStore;
use nova_files_trash_core::Trashes;

mod folders;
mod look;
mod operations;

pub struct Setup {
    pub home: Option<Home>,
    pub places: PlacesStore,
    pub gathered: GatheredStore,
    pub git: GitReader,
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
    pub folder_sizer: Arc<FolderSizer>,
    pub drives: DrivesStore,
    pub recent: RecentStore,
    pub folder_views: FolderViewsStore,
    pub inside: Arc<Inside>,
    pub seven: bool,
}

pub fn routes() -> Result<Routes<Setup, Client>, MessageType> {
    let mut routes = Routes::new();
    routes.on(|setup: &Setup, id, client: &Client, command: Command| {
        answer(id, client, command, setup)
    })?;
    routes.on(
        |setup: &Setup, id, client: &Client, command: DrivesCommand| {
            drives(id, client, command, setup)
        },
    )?;
    routes.on(|setup: &Setup, id, client: &Client, command| {
        match command {
            folders_api::Command::Views => setup.folder_views.listen(id, client.events()),
            folders_api::Command::SetView { folder, view } => setup.folder_views.set(folder, view),
        }
        Ok(())
    })?;
    routes.on(always(gathered))?;
    routes.on(|setup: &Setup, id, client: &Client, command| {
        match command {
            git_api::Command::Repos(folders) => setup.git.read(id, client.events(), folders),
            git_api::Command::Marks(folder) => setup.git.mark(id, client.events(), folder),
        }
        Ok(())
    })?;
    routes.on(|setup: &Setup, id, client: &Client, command| {
        match command {
            look_api::Command::Watch => setup.theme.listen(id, client.events()),
            look_api::Command::DesktopLook(look) => setup.theme.follow_desktop(look),
            look_api::Command::DesktopIconThemes(names) => setup.file_icons.follow_desktop(&names),
        }
        Ok(())
    })?;
    routes.on(always(open))?;
    routes.on(|setup: &Setup, id, client: &Client, command| {
        match command {
            places_api::Command::Places => setup.places.listen(id, client.events()),
            places_api::Command::ChangeBookmark(change) => setup.places.change(change),
        }
        Ok(())
    })?;
    routes.on(always(preview))?;
    routes.on(always(properties))?;
    routes.on(always(search))?;
    routes.on(|setup: &Setup, id, client: &Client, command| {
        match command {
            settings_api::Command::Watch => setup.settings.listen(id, client.events()),
            settings_api::Command::Change(change) => setup.settings.change(change),
        }
        Ok(())
    })?;
    routes.on(always(start))?;
    file_routes(&mut routes)?;
    routes.on_leave(forget);
    Ok(routes)
}

fn file_routes(routes: &mut Routes<Setup, Client>) -> Result<(), MessageType> {
    routes.on(|setup: &Setup, id, client: &Client, command| {
        operations::drop(id, client, command, setup);
        Ok(())
    })?;
    routes.on(|setup: &Setup, _, client: &Client, command| match command {
        archives_api::Command::Formats => client
            .events()
            .deliver(archives_api::Event::Formats { seven: setup.seven }),
        archives_api::Command::ArchiveOf { folder } => {
            let archive = within(&folder).map(|within| within.archive);
            client
                .events()
                .deliver(archives_api::Event::ArchiveOf { folder, archive })
        }
    })?;
    routes.on(|setup: &Setup, _, client: &Client, command| match command {
        trash_api::Command::List => {
            nova_files_trash_core::list(Trashes::of_home(setup.home.as_ref()), client.events())
        }
    })
}

type Route<M> = fn(&Setup, ClientId, &Client, M);

fn always<M>(
    route: Route<M>,
) -> impl Fn(&Setup, ClientId, &Client, M) -> Result<(), nova_files_api::ClientGone> {
    move |setup, id, client, command| {
        route(setup, id, client, command);
        Ok(())
    }
}

fn gathered(setup: &Setup, id: ClientId, client: &Client, command: gathered_api::Command) {
    match command {
        gathered_api::Command::Gathered => setup.gathered.listen(id, client.events()),
        gathered_api::Command::ChangeGathered(change) => setup.gathered.change(change),
        gathered_api::Command::ListGathered {
            id: listing,
            place,
            order,
        } => {
            let settings = setup.settings.current();
            let measured = settings.folder_sizes == FolderSizes::Measured;
            let sizer = measured.then_some(&setup.folder_sizer);
            let folders = setup.gathered.folders_of(place);
            let shown = (order, settings.hidden_files);
            nova_files_gathered_core::list(client.entries(), listing, folders, shown, sizer);
        }
    }
}

fn open(setup: &Setup, _: ClientId, client: &Client, command: open_api::Command) {
    match command {
        open_api::Command::OpenFile { path, activation } => {
            let archives: Arc<dyn TakeOut> = Arc::<Inside>::clone(&setup.inside);
            let opening = (Arc::clone(&setup.mime_types), archives);
            nova_files_open_core::open(client.events(), opening, path, activation);
        }
        open_api::Command::OpenWith(request) => {
            setup.open_with.answer(client.events(), request);
        }
        open_api::Command::OpenTerminal { folder, activation } => {
            nova_files_open_core::open_terminal(client.events(), folder, activation);
        }
    }
}

fn preview(setup: &Setup, _: ClientId, client: &Client, command: preview_api::Command) {
    match command {
        preview_api::Command::Preview { id, path, fit } => {
            let request = PreviewRequest {
                id,
                path,
                fit,
                current: Arc::clone(client.current_preview()),
                types: Arc::new(LazyMimes(Arc::clone(&setup.mime_types))),
                helper: setup.thumbnails.as_ref().map(|setup| setup.helper.clone()),
                player: setup
                    .thumbnails
                    .as_ref()
                    .and_then(|setup| nova_files_preview_core::player_beside(&setup.helper)),
            };
            nova_files_preview_core::answer(client.events(), request);
        }
        preview_api::Command::Page { id, index, width } => {
            let current = client.current_preview();
            nova_files_preview_core::page(client.events(), current, id, index, width);
        }
        preview_api::Command::Control { id, control } => {
            let current = client.current_preview();
            nova_files_preview_core::control(client.events().as_ref(), current, id, control);
        }
        preview_api::Command::Closed => client.current_preview().cancel(),
        preview_api::Command::OpenAt { path, at } => {
            let archives: Arc<dyn TakeOut> = Arc::<Inside>::clone(&setup.inside);
            let opening = (Arc::clone(&setup.mime_types), archives);
            nova_files_open_core::open_at(client.events(), opening, path, at);
        }
        preview_api::Command::Draw {
            id,
            document,
            picture,
            fit,
        } => {
            let current = client.current_preview();
            let helper = setup.thumbnails.as_ref().map(|setup| setup.helper.clone());
            let asked = nova_files_preview_core::DrawAsked {
                id,
                document,
                picture,
                fit,
            };
            nova_files_preview_core::draw(client.events(), current, helper, asked);
        }
    }
}

fn properties(setup: &Setup, id: ClientId, client: &Client, command: properties_api::Command) {
    let events = client.events();
    match command {
        properties_api::Command::Properties { id: asked, path } => {
            nova_files_properties_core::answer(events, asked, path);
        }
        properties_api::Command::Measure { id: scan, paths } => {
            setup.measurer.measure(id, events, scan, paths);
        }
        properties_api::Command::Space { id: scan, folder } => {
            setup.measurer.space(id, events, scan, folder);
        }
        properties_api::Command::StopMeasure(scan) => setup.measurer.stop(id, scan),
    }
}

fn search(setup: &Setup, _: ClientId, client: &Client, command: search_api::Command) {
    match command {
        search_api::Command::Search { id, query, limit } => {
            let asked = (client.events(), client.current_search());
            setup.searcher.search(asked, id, query, limit);
        }
        search_api::Command::Find { id, query, within } => {
            let asked = (client.events(), client.start_listing(id));
            setup.searcher.find(asked, id, query, within);
        }
        search_api::Command::Complete { typed, limit } => {
            let home = setup.home.as_ref().map(|home| home.folder.clone());
            nova_files_search_core::complete(client.events(), home, typed, limit);
        }
    }
}

fn start(setup: &Setup, _: ClientId, client: &Client, command: start_api::Command) {
    match command {
        start_api::Command::FolderUsed(folder) => setup.recent.used(folder),
        start_api::Command::RecentFolders { id, count, skip } => {
            let count = usize::try_from(count).unwrap_or(usize::MAX);
            setup.recent.answer(client.events(), id, count, skip);
        }
    }
}

pub fn routed_types() -> Result<Vec<MessageType>, MessageType> {
    Ok(routes()?.takes().collect())
}

pub fn emitted_types() -> Vec<MessageType> {
    vec![
        MessageType::of::<Event>(),
        MessageType::of::<archives_api::Event>(),
        MessageType::of::<DrivesEvent>(),
        MessageType::of::<folders_api::Event>(),
        MessageType::of::<gathered_api::Event>(),
        MessageType::of::<git_api::Event>(),
        MessageType::of::<look_api::Event>(),
        MessageType::of::<open_api::Event>(),
        MessageType::of::<places_api::Event>(),
        MessageType::of::<preview_api::Event>(),
        MessageType::of::<properties_api::Event>(),
        MessageType::of::<search_api::Event>(),
        MessageType::of::<settings_api::Event>(),
        MessageType::of::<start_api::Event>(),
        MessageType::of::<thumbnails_api::Event>(),
        MessageType::of::<transfer_api::Event>(),
        MessageType::of::<trash_api::Event>(),
    ]
}

pub fn run(receiver: &Receiver<CoreMessage>, idle_exit: Duration, setup: &Setup) {
    match routes() {
        Ok(routes) => serve(receiver, idle_exit, &routes, setup),
        Err(twice) => log::error!("two routes for {}", twice.name()),
    }
}

impl Served for Setup {
    type Client = Client;

    fn client(&self, events: Arc<dyn EventSink>) -> Client {
        let types = Arc::new(LazyMimes(Arc::clone(&self.mime_types)));
        Client::new(events, self.thumbnails.as_ref(), &self.file_icons, types)
    }

    fn busy(&self) -> bool {
        self.jobs.busy()
    }

    fn offered(&self, offer: Envelope) {
        match offer.open::<DriveOffer>() {
            Ok(DriveOffer(service)) => self.drives.offer(service),
            Err(other) => log::error!("no use for an offer of {}", other.kind().name()),
        }
    }
}

fn forget(setup: &Setup, client: ClientId) {
    setup.settings.forget(client);
    setup.theme.forget(client);
    setup.places.forget(client);
    setup.gathered.forget(client);
    setup.git.forget(client);
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
        command @ (Command::ListFolder { .. } | Command::Rows { .. }) => {
            folders::answer(client, command, setup);
            Ok(())
        }
        command @ (Command::WindowState | Command::SaveWindowState(_)) => {
            look::answer(client, command, setup)
        }
        command @ (Command::WatchJobs
        | Command::StartJob { .. }
        | Command::PauseJob(_)
        | Command::ResumeJob(_)
        | Command::ForgetJob(_)
        | Command::CancelJob(_)
        | Command::AnswerConflict { .. }
        | Command::Reply { .. }) => operations::answer(id, client, command, setup),
    }
}

fn drives(
    id: ClientId,
    client: &Client,
    command: DrivesCommand,
    setup: &Setup,
) -> Result<(), nova_files_api::ClientGone> {
    match command {
        DrivesCommand::Drives => setup.drives.listen(id, client.events()),
        DrivesCommand::Drive(action) => setup.drives.act(client.events(), action),
        DrivesCommand::DiskSpace(paths) => {
            disk_space::answer(client.events(), paths);
            Ok(())
        }
    }
}
