//! The program. Wiring only: it starts the core, the D-Bus service and the
//! window, and holds no logic of its own.
//!
//! `nova-files` opens a window. `nova-files --service`, which is how D-Bus
//! starts it for a launcher, runs only the core until something asks for a
//! window. Either way the first process claims the bus name; a later launch
//! asks that process for a window and exits. The process ends when the
//! core stops, which it does once idle (see `nova_files_core::IDLE_EXIT`).

#![forbid(unsafe_code)]

mod mode;

use std::error::Error;
use std::io::{self, Write as _};
use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use nova_files_api::Connector;
use nova_files_dbus::{Bus, Started};
use nova_files_ui::Request;

use crate::mode::Mode;

const CORE_WATCH_THREAD: &str = "nova-files-core-watch";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report(&format!("{error}"));
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mode = Mode::from_args(std::env::args_os().skip(1))?;
    let core = nova_files_core::spawn()?;
    let (requests, received) = mpsc::channel();

    let on_activate = {
        let requests = requests.clone();
        move || {
            requests.send(Request::OpenWindow).ok();
        }
    };
    let _service = match nova_files_dbus::start(&Bus::Session, core.connector.clone(), on_activate)
    {
        Ok(Started::Service(service)) => Some(service),
        Ok(Started::AlreadyRunning(running)) => {
            if mode == Mode::Window {
                running.activate()?;
            }
            return Ok(());
        }
        Err(error) if mode == Mode::Window => {
            // Still useful without a session bus: one window, no handover.
            report(&format!("no D-Bus session ({error}); running on its own"));
            None
        }
        Err(error) => return Err(error.into()),
    };

    watch_core(core.thread, requests)?;
    match mode {
        Mode::Window => nova_files_ui::run(core.connector, received),
        Mode::Service => wait_for_window(core.connector, received),
    }
    Ok(())
}

/// Service mode: no window and no GPU until one is asked for.
fn wait_for_window(connector: Connector, received: Receiver<Request>) {
    if received.recv() == Ok(Request::OpenWindow) {
        nova_files_ui::run(connector, received);
    }
}

/// Asks the UI to quit once the core thread ends, so the process ends with
/// the core.
fn watch_core(core: JoinHandle<()>, requests: Sender<Request>) -> io::Result<()> {
    thread::Builder::new()
        .name(CORE_WATCH_THREAD.to_owned())
        .spawn(move || {
            if core.join().is_err() {
                report("the core stopped unexpectedly");
            }
            requests.send(Request::Quit).ok();
        })?;
    Ok(())
}

/// Until there is a log, errors go to stderr.
fn report(message: &str) {
    writeln!(io::stderr(), "nova-files: {message}").ok();
}
