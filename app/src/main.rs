mod memory;
mod mode;
mod picker;

use std::cell::Cell;
use std::error::Error;
use std::io::{self, Write as _};
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::{Command, ExitCode};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use nova_files_api::{Connector, WindowRequest};
use nova_files_dbus::{Bus, Service, Started};
use nova_files_drives_api::DriveOffer;
use nova_files_ui::{Ended, Opening, RecoverGpu, Request, RequestSender, Requests};

use crate::mode::Mode;
use crate::picker::UiPicker;

const CORE_WATCH_THREAD: &str = "nova-files-core-watch";
const NO_CACHE_FOLDER: &str = "no cache folder: set XDG_CACHE_HOME or HOME";

const EVERY_DRIVER_VARIABLE: &str = "NOVA_FILES_EVERY_VULKAN_DRIVER";

static FILTER_SET: AtomicBool = AtomicBool::new(false);

fn main() -> ExitCode {
    for error in nova_files_core::start_traces() {
        report(&format!("{error}"));
    }
    skip_absent_vulkan_drivers();
    nova_files_core::set_give_back(memory::give_back);
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report(&format!("{error}"));
            ExitCode::FAILURE
        }
    }
}

const RESTART_NAME_WAIT: Duration = Duration::from_millis(500);

fn name_wait() -> Duration {
    if every_driver_asked() {
        RESTART_NAME_WAIT
    } else {
        Duration::ZERO
    }
}

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
        nova_files_core::launch_without(nova_files_core::FILTER_VARIABLE);
        FILTER_SET.store(true, Ordering::SeqCst);
    }
}

fn restart_with_every_driver(folder: Option<&Path>) -> Option<Command> {
    if !FILTER_SET.load(Ordering::SeqCst) {
        return None;
    }
    let mut command = Command::new(std::env::current_exe().ok()?);
    command
        .args(folder)
        .env_remove(nova_files_core::FILTER_VARIABLE)
        .env(EVERY_DRIVER_VARIABLE, "1");
    Some(command)
}

fn run() -> Result<(), Box<dyn Error>> {
    let mode = Mode::from_args(std::env::args_os().skip(1))?.resolve(std::env::current_dir)?;
    if let Mode::Index(folder) = &mode {
        let place = nova_files_core::IndexPlace::from_env().ok_or(NO_CACHE_FOLDER)?;
        return Ok(place.build(folder)?);
    }
    if mode == Mode::MakeDefault {
        nova_files_core::make_default_for_folders()?;
        let told = nova_files_dbus::find_running(&Bus::Session)
            .and_then(|running| running.map_or(Ok(()), |running| running.serve_file_manager()));
        if let Err(error) = told {
            report(&format!(
                "the running Nova Files answers \"Show in folder\" from its next start: {error}"
            ));
        }
        return Ok(());
    }
    let core = nova_files_core::spawn()?;
    core.connector
        .offer(DriveOffer(Arc::new(nova_files_dbus::Udisks::new(
            Bus::System,
        ))))?;
    let (requests, received) = nova_files_ui::channel();

    let on_open = {
        let requests = requests.clone();
        move |request| requests.send(Request::OpenWindow(request))
    };
    let started =
        nova_files_dbus::start_waiting(Bus::Session, core.connector.clone(), on_open, name_wait());
    let service = match started {
        Ok(Started::Service(mut service)) => {
            service.serve_file_picker(core.connector.clone(), UiPicker::new(requests.clone()));
            let asked = mode == Mode::FileManager;
            let should = move || asked || nova_files_core::opens_folders();
            if let Err(error) = service.serve_file_manager_when(should) {
                report(&format!("can't answer apps' \"Show in folder\": {error}"));
            }
            if let Err(error) = service.follow_desktop_look(core.connector.clone()) {
                report(&format!(
                    "can't follow the desktop's light or dark: {error}"
                ));
            }
            let quit = requests.clone();
            let watched = service.on_bus_lost(move || {
                report("the D-Bus session ended; stopping");
                quit.send(Request::Quit);
            });
            if let Err(error) = watched {
                report(&format!("can't watch the D-Bus session: {error}"));
            }
            Some(service)
        }
        Ok(Started::AlreadyRunning(running)) => {
            match &mode {
                Mode::Window(Some(folder)) => running.open(folder)?,
                Mode::Window(None) => running.activate()?,
                Mode::FileManager => running.serve_file_manager()?,
                Mode::Service | Mode::MakeDefault | Mode::Index(_) => {}
            }
            return Ok(());
        }
        Err(error) if !matches!(mode, Mode::Service | Mode::FileManager) => {
            report(&format!("no D-Bus session ({error}); running on its own"));
            None
        }
        Err(error) => return Err(error.into()),
    };

    watch_core(core.thread, requests.clone())?;
    show_windows(mode, &requests, &core.connector, &received, service);
    Ok(())
}

fn show_windows(
    mode: Mode,
    requests: &RequestSender,
    connector: &Connector,
    received: &Requests,
    service: Option<Service>,
) {
    let service = Cell::new(service);
    let restart: RecoverGpu = Rc::new(move |folder| {
        let Some(mut command) = restart_with_every_driver(folder) else {
            return;
        };
        if let Some(Err(error)) = service.take().as_ref().map(Service::release) {
            report(&format!("could not give up the bus name: {error}"));
        }
        report(&format!(
            "restarting with every Vulkan driver: {}",
            command.exec()
        ));
    });
    if let Mode::Window(folder) = mode {
        requests.send(Request::OpenWindow(WindowRequest {
            folder,
            select: Vec::new(),
        }));
    }
    loop {
        let first = match received.wait() {
            Some(Request::OpenWindow(request)) => Opening::Folder(request),
            Some(Request::Pick(request)) => Opening::Pick(request),
            Some(Request::ClosePick(_)) => continue,
            Some(Request::Quit) | None => break,
        };
        if nova_files_ui::run(connector, received, first, &restart, nova_files::parts())
            == Ended::Quit
        {
            break;
        }
    }
}

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

        let command = restart_with_every_driver(Some(Path::new("/srv/photos"))).unwrap();

        assert_eq!(command.get_program(), std::env::current_exe().unwrap());
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [OsStr::new("/srv/photos")]
        );
        let envs: Vec<_> = command.get_envs().collect();
        assert!(envs.contains(&(OsStr::new(nova_files_core::FILTER_VARIABLE), None)));
        assert!(envs.contains(&(OsStr::new(EVERY_DRIVER_VARIABLE), Some(OsStr::new("1")))));
        let command = restart_with_every_driver(None).unwrap();
        assert_eq!(command.get_args().count(), 0);
    }

    #[test]
    fn no_restart_when_this_process_filtered_nothing() {
        assert!(restart_with_every_driver(Some(Path::new("/srv/photos"))).is_none());
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
