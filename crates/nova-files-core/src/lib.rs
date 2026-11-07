//! The core. Index, search, folder listing, watching, jobs and settings. It
//! never draws anything.
//!
//! [`spawn`] starts the core on its own thread and returns the
//! [`Connector`] clients use to reach it. The core stops by itself once no
//! client is connected and no message has arrived for [`IDLE_EXIT`].

#![forbid(unsafe_code)]

mod details;
mod jobs;
mod list;
mod places;
mod run;
mod thumbs;

use std::io;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use nova_files_api::Connector;

pub use places::Home;
pub use thumbs::Thumbnails;

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
    spawn_with(idle_exit, Home::from_env(), Thumbnails::from_env())
}

/// Starts the core with its places under `home` and its thumbnails in
/// `thumbnails` instead of what the environment names, so tests never touch
/// the real ones. Without `thumbnails` the core makes none.
///
/// # Errors
/// When the OS refuses to start the core's thread.
pub fn spawn_with(
    idle_exit: Duration,
    home: Option<Home>,
    thumbnails: Option<Thumbnails>,
) -> io::Result<RunningCore> {
    let (connector, receiver) = nova_files_api::channel();
    let thread = thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || run::run(&receiver, idle_exit, home.as_ref(), thumbnails.as_ref()))?;
    Ok(RunningCore { connector, thread })
}
