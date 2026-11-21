mod jobs;
mod ops;
mod run;

use std::io;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use nova_files_api::{ClientId, Connector};
use nova_files_archives_core::inside;
use nova_files_entries_core::memory;

use nova_files_search_core::Searcher;
use nova_files_trash_core::Trashes;

pub use memory::set_give_back;
pub use nova_files_archives_core::{
    ArchiveEntry, ArchiveFailure, ArchiveKind, Archiver, Extracting, Flow, Recipe, seven_on,
};
pub use nova_files_disk::home::Home;
pub use nova_files_open_core::MimeTypes;
pub use nova_files_open_core::{
    NotDefault, launch_without, make_default_for_folders, opens_folders,
};
pub use nova_files_search_core::{IndexError, IndexPlace, Unusable};
pub use nova_files_thumbnails_core::Thumbnails;
pub use run::{emitted_types, routed_types};

pub const IDLE_EXIT: Duration = Duration::from_mins(3);

const THREAD_NAME: &str = "nova-files-core";
const SETTINGS_LISTENER: ClientId = ClientId(u64::MAX);

pub struct RunningCore {
    pub connector: Connector,
    pub thread: JoinHandle<()>,
}

pub fn spawn() -> io::Result<RunningCore> {
    spawn_with_idle_exit(IDLE_EXIT)
}

pub fn spawn_with_idle_exit(idle_exit: Duration) -> io::Result<RunningCore> {
    spawn_with_archiver(
        idle_exit,
        Home::from_env(),
        Thumbnails::from_env(),
        IndexPlace::from_env(),
        Archiver::from_env(),
    )
}

pub fn spawn_with(
    idle_exit: Duration,
    home: Option<Home>,
    thumbnails: Option<Thumbnails>,
    index: Option<IndexPlace>,
) -> io::Result<RunningCore> {
    spawn_with_archiver(idle_exit, home, thumbnails, index, None)
}

pub fn spawn_with_archiver(
    idle_exit: Duration,
    home: Option<Home>,
    thumbnails: Option<Thumbnails>,
    index: Option<IndexPlace>,
    archiver: Option<Archiver>,
) -> io::Result<RunningCore> {
    let (connector, receiver) = nova_files_api::channel();
    let searcher = Searcher::new(index, home.as_ref().map(|home| home.folder.clone()));
    let window_state =
        nova_files_disk::window_state::Store::open(home.as_ref().map(|home| home.state.as_path()));
    let seven = archiver.as_ref().is_some_and(Archiver::has_seven);
    let inside = Arc::new(inside::Inside::new(
        archiver.clone(),
        inside::opened_folder(std::env::var_os("XDG_RUNTIME_DIR")),
    ));
    let jobs = ops::jobs(
        home.as_ref().map(|home| home.state.as_path()),
        Trashes::of_home(home.as_ref()),
        connector.clone(),
        archiver,
    );
    let thread = thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            let config = home.as_ref().map(|home| home.config.as_path());
            let settings = nova_files_settings_core::Store::open(config);
            let theme = nova_files_look_core::theme::Store::open(config);
            settings.listen(SETTINGS_LISTENER, &theme.follower());
            let places = nova_files_places_core::Store::open(home.clone());
            let gathered = nova_files_gathered_core::Store::open(home.clone());
            let state_home = home.as_ref().map(|home| home.state.as_path());
            let recent = nova_files_start_core::Store::open(state_home);
            let folder_views = nova_files_folders_core::Store::open(state_home);
            let mime_types = Arc::default();
            let file_icons = Arc::new(nova_files_look_core::FileIcons::from_env(
                Arc::new(nova_files_open_core::LazyMimes(Arc::clone(&mime_types))),
                config,
            ));
            let setup = run::Setup {
                home,
                places,
                gathered,
                window_state,
                settings,
                theme,
                thumbnails,
                searcher,
                open_with: nova_files_open_core::OpenWithWorker::new(Arc::clone(&mime_types)),
                mime_types,
                file_icons,
                jobs: Arc::new(jobs),
                measurer: nova_files_properties_core::Measurer::default(),
                folder_sizer: Arc::new(nova_files_entries_core::FolderSizer::new(Arc::new(
                    nova_files_properties_core::FolderMeasure,
                ))),
                drives: nova_files_drives_core::Store::default(),
                recent,
                folder_views,
                inside,
                seven,
            };
            run::run(&receiver, idle_exit, &setup);
        })?;
    Ok(RunningCore { connector, thread })
}
