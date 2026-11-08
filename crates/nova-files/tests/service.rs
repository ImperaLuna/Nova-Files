//! The built program against a private D-Bus daemon, without a display:
//! service mode answers `Ping`, a later launch hands its window (and its
//! folder) to the first instance (or, in service mode, leaves it alone), and bad starts
//! exit with an error.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test setup should fail loudly"
)]

#[path = "../../nova-files-dbus/tests/support/mod.rs"]
mod support;

use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use nova_files_dbus::{APP_ID, FILES_INTERFACE, Started};
use support::{PrivateBus, WAIT};
use zbus::blocking::fdo::DBusProxy;
use zbus::blocking::{Connection, Proxy};

/// How long a test waits for something that should not happen.
const QUIET: Duration = Duration::from_millis(500);

/// The program, pointed at `address` as its session bus and with no
/// display, so a window it should not open fails instead of showing.
fn program(address: &str, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_nova-files"));
    command
        .args(args)
        .env("DBUS_SESSION_BUS_ADDRESS", address)
        .env_remove(nova_files_core::TRACE_VARIABLE)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    command
}

/// Runs the program to its end, failing the test if it takes longer than
/// [`WAIT`].
fn finish(mut command: Command) -> Output {
    let child = command.spawn().unwrap();
    let (sender, output) = mpsc::channel();
    thread::spawn(move || sender.send(child.wait_with_output()).ok());
    output
        .recv_timeout(WAIT)
        .expect("the program exits")
        .unwrap()
}

fn client(bus: &PrivateBus) -> Connection {
    zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .method_timeout(WAIT)
        .build()
        .unwrap()
}

/// A running program, killed when dropped.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.kill().ok();
        self.0.wait().ok();
    }
}

/// Starts the program in service mode and waits until it owns the name.
fn start_service(bus: &PrivateBus) -> Running {
    let watcher = client(bus);
    let owners = DBusProxy::new(&watcher)
        .unwrap()
        .receive_name_owner_changed_with_args(&[(0, APP_ID)])
        .unwrap();
    let running = Running(program(&bus.address, &["--service"]).spawn().unwrap());
    let (sender, owned) = mpsc::channel();
    thread::spawn(move || {
        for _ in owners {
            sender.send(()).ok();
        }
    });
    owned
        .recv_timeout(WAIT)
        .expect("the service takes its name");
    running
}

#[test]
fn service_mode_answers_ping() {
    let bus = PrivateBus::start();
    let _service = start_service(&bus);

    let connection = client(&bus);
    let files = Proxy::new(
        &connection,
        APP_ID,
        nova_files_dbus::object_path().unwrap(),
        FILES_INTERFACE,
    )
    .unwrap();
    files.call::<_, _, ()>("Ping", &()).unwrap();
}

#[test]
fn second_service_start_exits_at_once() {
    let bus = PrivateBus::start();
    let _service = start_service(&bus);

    assert!(
        finish(program(&bus.address, &["--service"]))
            .status
            .success()
    );
}

#[test]
fn second_launch_hands_its_window_to_the_first() {
    let bus = PrivateBus::start();
    let (activations, activated) = mpsc::channel();
    let core = nova_files_core::spawn().unwrap();
    let first = nova_files_dbus::start(bus.bus(), core.connector, move |folder| {
        activations.send(folder).ok();
    })
    .unwrap();
    assert!(matches!(first, Started::Service(_)));

    assert!(finish(program(&bus.address, &[])).status.success());
    assert_eq!(activated.recv_timeout(WAIT), Ok(None));
}

#[test]
fn second_launch_with_a_folder_hands_it_to_the_first() {
    let bus = PrivateBus::start();
    let (opened, received) = mpsc::channel();
    let core = nova_files_core::spawn().unwrap();
    let _first = nova_files_dbus::start(bus.bus(), core.connector, move |folder| {
        opened.send(folder).ok();
    })
    .unwrap();

    let launch = finish(program(&bus.address, &["/tmp/some folder"]));

    assert!(launch.status.success());
    assert_eq!(
        received.recv_timeout(WAIT),
        Ok(Some(PathBuf::from("/tmp/some folder")))
    );
}

#[test]
fn second_service_start_asks_for_no_window() {
    let bus = PrivateBus::start();
    let (activations, activated) = mpsc::channel();
    let core = nova_files_core::spawn().unwrap();
    let _first = nova_files_dbus::start(bus.bus(), core.connector, move |folder| {
        activations.send(folder).ok();
    })
    .unwrap();

    assert!(
        finish(program(&bus.address, &["--service"]))
            .status
            .success()
    );
    assert!(activated.recv_timeout(QUIET).is_err());
}

#[test]
fn service_mode_without_a_bus_fails() {
    let output = finish(program(
        "unix:path=/nonexistent/nova-files-bus",
        &["--service"],
    ));
    assert!(!output.status.success());
}

#[test]
fn unknown_flag_is_refused_with_the_usage() {
    let output = finish(program(
        "unix:path=/nonexistent/nova-files-bus",
        &["--frobnicate"],
    ));
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unexpected argument --frobnicate"),
        "{stderr}"
    );
}

/// A start that ends at once (a bad flag), with the start-up trace going to
/// `trace`.
fn traced_start(trace: &std::path::Path) -> Output {
    let mut command = program("unix:path=/nonexistent/nova-files-bus", &["--frobnicate"]);
    command.env(nova_files_core::TRACE_VARIABLE, trace);
    finish(command)
}

#[test]
fn the_start_up_trace_records_the_process_start() {
    let trace = std::env::temp_dir().join(format!("nova-files-trace-{}", std::process::id()));
    std::fs::remove_file(&trace).ok();
    traced_start(&trace);
    let written = std::fs::read_to_string(&trace).unwrap();
    std::fs::remove_file(&trace).ok();
    let labels: Vec<&str> = written
        .lines()
        .filter_map(|line| line.split_once(' ').map(|(_, label)| label))
        .collect();
    assert_eq!(labels, ["process-start"]);
}

#[test]
fn a_trace_that_cant_be_written_is_reported_and_the_start_goes_on() {
    let output = traced_start(std::path::Path::new("/nonexistent/folder/trace"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("start-up trace"), "{stderr}");
    assert!(
        stderr.contains("unexpected argument --frobnicate"),
        "{stderr}"
    );
}
