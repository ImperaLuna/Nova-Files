#![forbid(unsafe_code)]

mod accounts;
mod archive;
mod disk_space;
mod drives;
mod file_icons;
mod folder_views;
mod gathered;
mod icon_theme;
mod index;
mod inside;
mod jobs;
mod measure;
mod ops;
mod places;
mod preview;
mod properties;
mod recent;
mod run;
mod search;
mod theme;
mod thumbs;
mod trace;
mod trash;
mod vulkan_drivers;
mod window_state;

use std::io;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use nova_files_api::Connector;
use nova_files_disk::{
    capped, change_writer, config_watch, hand_edited, mount_root, private_file, saver, timed,
};
use nova_files_entries::{list, memory, order};

use search::Searcher;

pub use archive::{
    ArchiveEntry, ArchiveFailure, ArchiveKind, Archiver, Extracting, Flow, Recipe, seven_on,
};
pub use index::{IndexError, IndexPlace, Unusable};
pub use memory::set_give_back;
pub use nova_files_open_core::MimeTypes;
pub use nova_files_open_core::{NotDefault, make_default_for_folders, opens_folders};
pub use places::Home;
pub use run::{emitted_types, routed_types};
pub use thumbs::Thumbnails;
pub use trace::{FRAME_TRACE_VARIABLE, STARTUP_TRACE_VARIABLE, TraceError, start_traces};
pub use vulkan_drivers::{FILTER_VARIABLE, absent_vendor_drivers};

pub const IDLE_EXIT: Duration = Duration::from_mins(3);

const THREAD_NAME: &str = "nova-files-core";

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
            let theme = theme::Store::open(config);
            theme.follow(&settings);
            let places = places::Store::open(home.clone());
            let gathered = gathered::Store::open(home.clone());
            let state_home = home.as_ref().map(|home| home.state.as_path());
            let recent = recent::Store::open(state_home);
            let folder_views = folder_views::Store::open(state_home);
            let mime_types = Arc::default();
            let file_icons = Arc::new(file_icons::FileIcons::from_env(
                Arc::clone(&mime_types),
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
                drives: drives::Store::default(),
                recent,
                folder_views,
                inside,
            };
            run::run(&receiver, idle_exit, &setup);
        })?;
    Ok(RunningCore { connector, thread })
}
