//! The core. Index, search, folder listing, watching, jobs, settings and theme. It
//! never draws anything.
//!
//! [`spawn`] starts the core on its own thread and returns the
//! [`Connector`] clients use to reach it. The core stops by itself once no
//! client is connected and no message has arrived for [`IDLE_EXIT`].

#![forbid(unsafe_code)]

mod accounts;
mod config_watch;
mod details;
mod details_queue;
mod hand_edited;
mod index;
mod jobs;
mod list;
mod memory;
mod mime;
mod open;
mod ops;
mod places;
mod private_file;
mod run;
mod search;
mod settings;
mod theme;
mod thumbs;
mod trace;
mod vulkan_drivers;
mod window_state;

use std::io;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use nova_files_api::Connector;

use search::Searcher;

pub use index::{IndexError, IndexPlace, Unusable};
pub use memory::set_give_back;
pub use mime::MimeTypes;
pub use places::Home;
pub use thumbs::Thumbnails;
pub use trace::{FRAME_TRACE_VARIABLE, STARTUP_TRACE_VARIABLE, TraceError, start_traces};
pub use vulkan_drivers::{FILTER_VARIABLE, absent_vendor_drivers};

/// How long the core waits, with no client connected and no message, before
/// it stops. Fixed by design, not a setting.
pub const IDLE_EXIT: Duration = Duration::from_mins(3);

const THREAD_NAME: &str = "nova-files-core";

/// The running core: how to reach it and how to wait for it to stop.
pub struct RunningCore {
    pub connector: Connector,
    pub thread: JoinHandle<()>,
}

/// Starts the core with the standard [`IDLE_EXIT`].
///
/// # Errors
/// When the OS refuses to start the core's thread.
pub fn spawn() -> io::Result<RunningCore> {
    spawn_with_idle_exit(IDLE_EXIT)
}

/// Starts the core, which stops after `idle_exit` without clients or
/// messages. Tests use a short `idle_exit`; the program uses [`spawn`].
///
/// # Errors
/// When the OS refuses to start the core's thread.
pub fn spawn_with_idle_exit(idle_exit: Duration) -> io::Result<RunningCore> {
    spawn_with(
        idle_exit,
        Home::from_env(),
        Thumbnails::from_env(),
        IndexPlace::from_env(),
    )
}

/// Starts the core with the given folders instead of what the environment
/// names, so tests never touch the real ones.
///
/// Places come from `home`, thumbnails go to `thumbnails` and the index of
/// `home` to `index`. Without `thumbnails` the core makes none; without
/// `home` or `index` it can't search.
///
/// # Errors
/// When the OS refuses to start the core's thread.
pub fn spawn_with(
    idle_exit: Duration,
    home: Option<Home>,
    thumbnails: Option<Thumbnails>,
    index: Option<IndexPlace>,
) -> io::Result<RunningCore> {
    let (connector, receiver) = nova_files_api::channel();
    let searcher = Searcher::new(index, home.as_ref().map(|home| home.folder.clone()));
    let window_state = window_state::Store::open(home.as_ref().map(|home| home.state.as_path()));
    let jobs = ops::Jobs::new(
        home.as_ref().map(|home| home.state.as_path()),
        connector.clone(),
    );
    let thread = thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            // Read on the core's thread, so a slow disk never holds up the
            // caller (the window, at start-up).
            let config = home.as_ref().map(|home| home.config.as_path());
            let settings = settings::Store::open(config);
            let theme = theme::Store::open(config);
            theme.follow(&settings);
            let setup = run::Setup {
                home,
                window_state,
                settings,
                theme,
                thumbnails,
                searcher,
                mime_types: Arc::default(),
                jobs,
            };
            run::run(&receiver, idle_exit, &setup);
        })?;
    Ok(RunningCore { connector, thread })
}
