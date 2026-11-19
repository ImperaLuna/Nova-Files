#![forbid(unsafe_code)]

mod accounts;
mod gathered;
mod inside;
mod jobs;
mod measure;
mod ops;
mod places;
mod properties;
mod recent;
mod run;
mod trace;
mod trash;
mod vulkan_drivers;
mod window_state;

use std::io;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use nova_files_api::{ClientId, Connector};
use nova_files_disk::{
    change_writer, config_watch, hand_edited, mount_root, private_file, saver, timed,
};
use nova_files_entries_core::{list, memory, order};

use nova_files_search_core::Searcher;

pub use memory::set_give_back;
pub use nova_files_archives_core::{
    ArchiveEntry, ArchiveFailure, ArchiveKind, Archiver, Extracting, Flow, Recipe, seven_on,
};
pub use nova_files_open_core::MimeTypes;
pub use nova_files_open_core::{NotDefault, make_default_for_folders, opens_folders};
pub use nova_files_search_core::{IndexError, IndexPlace, Unusable};
pub use nova_files_thumbnails_core::Thumbnails;
pub use places::Home;
pub use run::{emitted_types, routed_types};
pub use trace::{FRAME_TRACE_VARIABLE, STARTUP_TRACE_VARIABLE, TraceError, start_traces};
pub use vulkan_drivers::{FILTER_VARIABLE, absent_vendor_drivers};

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
    let window_state = window_state::Store::open(home.as_ref().map(|home| home.state.as_path()));
    let inside = Arc::new(inside::Inside::new(
        archiver.clone(),
        inside::opened_folder(std::env::var_os("XDG_RUNTIME_DIR")),
    ));
    let jobs = ops::jobs(
        home.as_ref().map(|home| home.state.as_path()),
        trash::trashes(home.as_ref()),
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
            let places = places::Store::open(home.clone());
            let gathered = gathered::Store::open(home.clone());
            let state_home = home.as_ref().map(|home| home.state.as_path());
            let recent = recent::Store::open(state_home);
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
                measurer: measure::Measurer::default(),
                drives: nova_files_drives_core::Store::default(),
                recent,
                folder_views,
                inside,
            };
            run::run(&receiver, idle_exit, &setup);
        })?;
    Ok(RunningCore { connector, thread })
}
