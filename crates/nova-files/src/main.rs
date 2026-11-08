//! The program. Wiring only: it starts the core, the D-Bus service and the
//! window, and holds no logic of its own.
//!
//! `nova-files` opens a window on the home folder, `nova-files <folder>` on
//! that folder. `nova-files --service`, which is how D-Bus
//! starts it for a launcher, runs only the core until something asks for a
//! window. Either way the first process claims the bus name; a later launch
//! asks that process for a window (on its folder) and exits.
//!
//! Windows come and go: GPUI ends its run when the last one closes, and the
//! process carries on without a GPU, waiting for the next window request.
//! The process ends when the core stops, which it does once idle (see
//! `nova_files_core::IDLE_EXIT`).
//!
//! With `NOVA_FILES_STARTUP_TRACE` set, it first starts the start-up trace
//! (see `nova_files_core::start_startup_trace`).
//!
//! Then, while it is the only thread, it tells the Vulkan
//! loader to skip drivers for GPUs the PC doesn't have (see
//! `nova_files_core::absent_vendor_drivers`); each skipped driver is a set of
//! libraries a cold start no longer reads. If the first window then can't
//! set up its GPU, the program starts again on that folder with every
//! driver (see `restart_with_every_driver`).

mod mode;

use std::cell::Cell;
use std::error::Error;
use std::io::{self, Write as _};
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::{Command, ExitCode};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use nova_files_api::Connector;
use nova_files_dbus::{Bus, Service, Started};
use nova_files_ui::{Ended, RecoverGpu, Request, RequestSender, Requests};

use crate::mode::Mode;

const CORE_WATCH_THREAD: &str = "nova-files-core-watch";

/// Set on a restart that gives the Vulkan loader every driver, so the new
/// process doesn't filter them again.
const EVERY_DRIVER_VARIABLE: &str = "NOVA_FILES_EVERY_VULKAN_DRIVER";

/// Whether this process set the Vulkan driver filter. A filter the user set
/// is never taken back.
static FILTER_SET: AtomicBool = AtomicBool::new(false);

fn main() -> ExitCode {
    if let Err(error) = nova_files_core::start_startup_trace() {
        report(&format!("start-up trace: {error}"));
    }
    skip_absent_vulkan_drivers();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report(&format!("{error}"));
            ExitCode::FAILURE
        }
    }
}

/// How long a restarted process waits for the bus to free its old name
/// before it takes the name as another instance's.
const RESTART_NAME_WAIT: Duration = Duration::from_millis(500);

/// How long to wait for the bus name: only a restart waits.
fn name_wait() -> Duration {
    if every_driver_asked() {
        RESTART_NAME_WAIT
    } else {
        Duration::ZERO
    }
}

/// Whether this process is a restart that must keep every driver.
fn every_driver_asked() -> bool {
    std::env::var_os(EVERY_DRIVER_VARIABLE).is_some()
}

#[expect(
    unsafe_code,
    reason = "setting a variable is sound only while no other thread exists, and main calls this first"
)]
fn skip_absent_vulkan_drivers() {
    if every_driver_asked() {
        return;
    }
    if let Some(filter) = nova_files_core::absent_vendor_drivers() {
        // SAFETY: called first thing in `main`, before the core, D-Bus or
        // GPUI start a thread, so nothing can read the environment meanwhile.
        unsafe { std::env::set_var(nova_files_core::FILTER_VARIABLE, filter) };
        FILTER_SET.store(true, Ordering::SeqCst);
    }
}

/// The command that starts this program again on `folder` with every
/// Vulkan driver, or `None` when this process didn't filter any.
///
/// A window that can't set up its GPU with drivers filtered may still draw
/// with the software driver (lavapipe): after an NVIDIA driver update
/// without a reboot, NVIDIA's Vulkan and OpenGL both fail. The environment
/// can't be changed once threads run, so the new process gets it instead.
fn restart_with_every_driver(folder: &Path) -> Option<Command> {
    if !FILTER_SET.load(Ordering::SeqCst) {
        return None;
    }
    let mut command = Command::new(std::env::current_exe().ok()?);
    command
        .arg(folder)
        .env_remove(nova_files_core::FILTER_VARIABLE)
        .env(EVERY_DRIVER_VARIABLE, "1");
    Some(command)
}

fn run() -> Result<(), Box<dyn Error>> {
    let mode = Mode::from_args(std::env::args_os().skip(1))?.resolve(std::env::current_dir)?;
    let core = nova_files_core::spawn()?;
    let (requests, received) = nova_files_ui::channel();

    let on_open = {
        let requests = requests.clone();
        move |folder| requests.send(Request::OpenWindow(folder))
    };
    let started =
        nova_files_dbus::start_waiting(Bus::Session, core.connector.clone(), on_open, name_wait());
    let service = match started {
        Ok(Started::Service(service)) => Some(service),
        Ok(Started::AlreadyRunning(running)) => {
            match &mode {
                Mode::Window(Some(folder)) => running.open(folder)?,
                Mode::Window(None) => running.activate()?,
                Mode::Service => {}
            }
            return Ok(());
        }
        Err(error) if mode != Mode::Service => {
            // Still useful without a session bus: windows, no handover.
            report(&format!("no D-Bus session ({error}); running on its own"));
            None
        }
        Err(error) => return Err(error.into()),
    };

    watch_core(core.thread, requests.clone())?;
    show_windows(mode, &requests, &core.connector, &received, service);
    Ok(())
}

/// Opens the first window in window mode. Then each `OpenWindow` while no
/// window is open starts the UI again, until something asks it to quit.
///
/// Holds the bus name (`service`) until the program ends or restarts.
fn show_windows(
    mode: Mode,
    requests: &RequestSender,
    connector: &Connector,
    received: &Requests,
    service: Option<Service>,
) {
    let service = Cell::new(service);
    // Replaces this process, so it returns only when that failed.
    let restart: RecoverGpu = Rc::new(move |folder| {
        let Some(mut command) = restart_with_every_driver(folder) else {
            return;
        };
        // The bus confirms the release, so the new process finds the name
        // free.
        if let Some(Err(error)) = service.take().map(Service::release) {
            report(&format!("could not give up the bus name: {error}"));
        }
        report(&format!(
            "restarting with every Vulkan driver: {}",
            command.exec()
        ));
    });
    if let Mode::Window(folder) = mode {
        requests.send(Request::OpenWindow(folder));
    }
    while let Some(Request::OpenWindow(folder)) = received.wait() {
        if nova_files_ui::run(connector, received, folder, &restart) == Ended::Quit {
            break;
        }
    }
}

/// Asks the UI to quit once the core thread ends, so the process ends with
/// the core.
fn watch_core(core: JoinHandle<()>, requests: RequestSender) -> io::Result<()> {
    thread::Builder::new()
        .name(CORE_WATCH_THREAD.to_owned())
        .spawn(move || {
            if core.join().is_err() {
                report("the core stopped unexpectedly");
            }
            requests.send(Request::Quit);
        })?;
    Ok(())
}

/// Until there is a log, errors go to stderr.
fn report(message: &str) {
    writeln!(io::stderr(), "nova-files: {message}").ok();
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    #[test]
    fn a_restart_gives_back_the_drivers_this_process_filtered() {
        FILTER_SET.store(true, Ordering::SeqCst);

        let command = restart_with_every_driver(Path::new("/srv/photos")).unwrap();

        assert_eq!(command.get_program(), std::env::current_exe().unwrap());
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [OsStr::new("/srv/photos")]
        );
        let envs: Vec<_> = command.get_envs().collect();
        assert!(envs.contains(&(OsStr::new(nova_files_core::FILTER_VARIABLE), None)));
        assert!(envs.contains(&(OsStr::new(EVERY_DRIVER_VARIABLE), Some(OsStr::new("1")))));
    }

    #[test]
    fn no_restart_when_this_process_filtered_nothing() {
        assert!(restart_with_every_driver(Path::new("/srv/photos")).is_none());
    }

    #[expect(
        unsafe_code,
        reason = "nextest runs each test in a process of its own, with no other thread reading"
    )]
    #[test]
    fn a_restart_keeps_every_driver_and_waits_for_its_name() {
        assert!(!every_driver_asked());
        assert_eq!(name_wait(), Duration::ZERO);
        // SAFETY: see the reason above.
        unsafe { std::env::set_var(EVERY_DRIVER_VARIABLE, "1") };
        assert!(every_driver_asked());
        assert_eq!(name_wait(), RESTART_NAME_WAIT);
    }
}
