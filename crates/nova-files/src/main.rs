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
//! libraries a cold start no longer reads. If a window then can't set up
//! its GPU, the program gives the loader every driver back and tries once
//! more (see `restore_vulkan_drivers`).

mod mode;

use std::error::Error;
use std::io::{self, Write as _};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

use nova_files_api::Connector;
use nova_files_dbus::{Bus, Started};
use nova_files_ui::{Ended, Request, RequestSender, Requests};

use crate::mode::Mode;

const CORE_WATCH_THREAD: &str = "nova-files-core-watch";

/// Whether this process set the Vulkan driver filter, so it may take it
/// back. A filter the user set is never touched.
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

#[expect(
    unsafe_code,
    reason = "setting a variable is sound only while no other thread exists, and main calls this first"
)]
fn skip_absent_vulkan_drivers() {
    if let Some(filter) = nova_files_core::absent_vendor_drivers() {
        // SAFETY: called first thing in `main`, before the core, D-Bus or
        // GPUI start a thread, so nothing can read the environment meanwhile.
        unsafe { std::env::set_var(nova_files_core::FILTER_VARIABLE, filter) };
        FILTER_SET.store(true, Ordering::SeqCst);
    }
}

/// A window's GPU setup failed: gives the loader every driver back.
///
/// With drivers filtered, the next try can then use the software driver
/// (lavapipe):
/// after an NVIDIA driver update without a reboot, NVIDIA's Vulkan and
/// OpenGL both fail, and lavapipe is what still draws. Returns whether it
/// changed anything; only the first call can.
#[expect(
    unsafe_code,
    reason = "removing a variable while other threads run; see the SAFETY note"
)]
fn restore_vulkan_drivers() -> bool {
    if !FILTER_SET.swap(false, Ordering::SeqCst) {
        return false;
    }
    // SAFETY: other threads exist by now. glibc's unsetenv shifts the
    // environment in place, under its own lock, without freeing or moving
    // the array, so a thread reading the environment meanwhile sees a valid
    // array and may at worst miss one variable. This runs at most once,
    // and only after the GPU failed.
    unsafe { std::env::remove_var(nova_files_core::FILTER_VARIABLE) };
    true
}

fn run() -> Result<(), Box<dyn Error>> {
    let mode = Mode::from_args(std::env::args_os().skip(1))?.resolve(std::env::current_dir)?;
    let core = nova_files_core::spawn()?;
    let (requests, received) = nova_files_ui::channel();

    let on_open = {
        let requests = requests.clone();
        move |folder| requests.send(Request::OpenWindow(folder))
    };
    let _service = match nova_files_dbus::start(Bus::Session, core.connector.clone(), on_open) {
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
    show_windows(mode, &requests, &core.connector, &received);
    Ok(())
}

/// Opens the first window in window mode. Then each `OpenWindow` while no
/// window is open starts the UI again, until something asks it to quit.
fn show_windows(mode: Mode, requests: &RequestSender, connector: &Connector, received: &Requests) {
    if let Mode::Window(folder) = mode {
        requests.send(Request::OpenWindow(folder));
    }
    while let Some(Request::OpenWindow(folder)) = received.wait() {
        if nova_files_ui::run(connector, received, folder, restore_vulkan_drivers) == Ended::Quit {
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
    use super::*;

    #[expect(
        unsafe_code,
        reason = "nextest runs each test in a process of its own, with no other thread reading"
    )]
    fn set_filter(value: &str) {
        // SAFETY: see the reason above.
        unsafe { std::env::set_var(nova_files_core::FILTER_VARIABLE, value) };
    }

    #[test]
    fn a_filter_this_process_set_is_given_back_once() {
        set_filter("lvp_icd.x86_64.json");
        FILTER_SET.store(true, Ordering::SeqCst);

        assert!(restore_vulkan_drivers());
        assert_eq!(std::env::var_os(nova_files_core::FILTER_VARIABLE), None);
        assert!(!restore_vulkan_drivers());
    }

    #[test]
    fn a_filter_the_user_set_stays() {
        set_filter("radeon_icd.x86_64.json");

        assert!(!restore_vulkan_drivers());
        assert_eq!(
            std::env::var_os(nova_files_core::FILTER_VARIABLE),
            Some("radeon_icd.x86_64.json".into())
        );
    }
}
