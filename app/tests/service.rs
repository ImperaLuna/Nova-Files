#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test setup should fail loudly"
)]

#[path = "../../fronts/dbus/tests/support/mod.rs"]
mod support;

use std::io;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use nova_files_dbus::{APP_ID, FILE_MANAGER_BUS_NAME, FILES_INTERFACE, Started};
use nova_files_test_folder::TestFolder;
use support::{PrivateBus, WAIT};
use zbus::blocking::fdo::DBusProxy;
use zbus::blocking::{Connection, Proxy};

const QUIET: Duration = Duration::from_millis(500);

fn program(address: &str, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_nova-files"));
    command
        .args(args)
        .env("DBUS_SESSION_BUS_ADDRESS", address)
        .env_remove(nova_files::STARTUP_TRACE_VARIABLE)
        .env_remove(nova_files::FRAME_TRACE_VARIABLE)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    command
}

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

struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.kill().ok();
        self.0.wait().ok();
    }
}

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
fn service_mode_exits_when_its_bus_stops() {
    let bus = PrivateBus::start();
    let mut service = start_service(&bus);
    let stderr = service.0.stderr.take().unwrap();
    let (sender, closed) = mpsc::channel();
    thread::spawn(move || {
        io::read_to_string(stderr).ok();
        sender.send(()).ok();
    });

    drop(bus);

    closed.recv_timeout(WAIT).expect("the service exits");
    assert!(service.0.wait().unwrap().success());
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
    let core = nova_files::spawn().unwrap();
    let first = nova_files_dbus::start(bus.bus(), core.connector, move |request| {
        activations.send(request.folder).ok();
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
    let core = nova_files::spawn().unwrap();
    let _first = nova_files_dbus::start(bus.bus(), core.connector, move |request| {
        opened.send(request.folder).ok();
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
    let core = nova_files::spawn().unwrap();
    let _first = nova_files_dbus::start(bus.bus(), core.connector, move |request| {
        activations.send(request.folder).ok();
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
fn a_restart_takes_the_name_its_old_process_gives_up() {
    let bus = PrivateBus::start();
    let core = nova_files::spawn().unwrap();
    let Started::Service(old) = nova_files_dbus::start(bus.bus(), core.connector, |_| {}).unwrap()
    else {
        panic!("the old process owns the name");
    };
    let watcher = client(&bus);
    let owners = DBusProxy::new(&watcher)
        .unwrap()
        .receive_name_owner_changed_with_args(&[(0, APP_ID)])
        .unwrap();
    let (sender, owned) = mpsc::channel();
    thread::spawn(move || {
        for change in owners {
            if change.args().unwrap().new_owner().is_some() {
                sender.send(()).ok();
            }
        }
    });

    let mut restart = program(&bus.address, &["--service"]);
    restart.env("NOVA_FILES_EVERY_VULKAN_DRIVER", "1");
    let _restarted = Running(restart.spawn().unwrap());
    assert!(owned.recv_timeout(QUIET).is_err());
    old.release().unwrap();

    owned
        .recv_timeout(WAIT)
        .expect("the restarted process takes the name");
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
fn a_window_without_a_bus_runs_on_its_own() {
    let mut window = Running(
        program("unix:path=/nonexistent/nova-files-bus", &["/tmp"])
            .spawn()
            .unwrap(),
    );
    let stderr = window.0.stderr.take().unwrap();
    let (sender, lines) = mpsc::channel();
    thread::spawn(move || {
        for line in io::BufRead::lines(io::BufReader::new(stderr)) {
            sender.send(line.unwrap_or_default()).ok();
        }
    });

    let said = lines
        .recv_timeout(WAIT)
        .expect("the program reports the missing bus");
    assert!(said.contains("running on its own"), "{said}");
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

fn traced_start(trace: &std::path::Path) -> Output {
    let mut command = program("unix:path=/nonexistent/nova-files-bus", &["--frobnicate"]);
    command.env(nova_files::STARTUP_TRACE_VARIABLE, trace);
    finish(command)
}

#[test]
fn the_start_up_trace_records_the_process_start() {
    let folder = TestFolder::new("trace");
    let trace = folder.join("trace");
    traced_start(&trace);
    let written = std::fs::read_to_string(&trace).unwrap();
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

#[test]
fn a_frame_trace_that_cant_be_written_is_reported_and_the_start_goes_on() {
    let mut command = program("unix:path=/nonexistent/nova-files-bus", &["--frobnicate"]);
    command.env(
        nova_files::FRAME_TRACE_VARIABLE,
        "/nonexistent/folder/frames",
    );
    let output = finish(command);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("frame trace"), "{stderr}");
    assert!(
        stderr.contains("unexpected argument --frobnicate"),
        "{stderr}"
    );
}

#[test]
fn make_default_makes_nova_files_open_folders() {
    let folder = TestFolder::new("default");
    let root = folder.join("root");
    let (config, data) = (root.join("config"), root.join("data"));
    let make_default = || {
        let mut command = program("unix:path=/nonexistent/nova-files-bus", &["--make-default"]);
        command
            .env("XDG_CONFIG_HOME", &config)
            .env("XDG_DATA_HOME", &data)
            .env("XDG_DATA_DIRS", root.join("none"))
            .env("XDG_CONFIG_DIRS", root.join("none"))
            .env_remove("XDG_CURRENT_DESKTOP");
        finish(command)
    };

    let refused = make_default();
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("install Nova Files first"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );

    let entry = data.join(format!("applications/{APP_ID}.desktop"));
    std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
    std::fs::write(
        &entry,
        "[Desktop Entry]\nType=Application\nExec=nova-files %U\n",
    )
    .unwrap();
    let made = make_default();
    let list = std::fs::read_to_string(config.join("mimeapps.list")).unwrap_or_default();

    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    assert!(
        list.contains(&format!("inode/directory={APP_ID}.desktop")),
        "{list}"
    );
}

struct Folders(TestFolder);

impl Folders {
    fn new(name: &str, default: bool) -> Self {
        let root = TestFolder::new(name);
        let entry = root.join(format!("data/applications/{APP_ID}.desktop"));
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::fs::write(
            &entry,
            "[Desktop Entry]\nType=Application\nExec=nova-files %U\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("config")).unwrap();
        if default {
            std::fs::write(
                root.join("config/mimeapps.list"),
                format!("[Default Applications]\ninode/directory={APP_ID}.desktop\n"),
            )
            .unwrap();
        }
        Self(root)
    }

    fn apply(&self, command: &mut Command) {
        command
            .env("XDG_CONFIG_HOME", self.0.join("config"))
            .env("XDG_DATA_HOME", self.0.join("data"))
            .env("XDG_CONFIG_DIRS", self.0.join("none"))
            .env("XDG_DATA_DIRS", self.0.join("none"))
            .env_remove("XDG_CURRENT_DESKTOP");
    }
}

fn file_manager_owned(bus: &PrivateBus) -> mpsc::Receiver<()> {
    let watcher = client(bus);
    let owners = DBusProxy::new(&watcher)
        .unwrap()
        .receive_name_owner_changed_with_args(&[(0, FILE_MANAGER_BUS_NAME)])
        .unwrap();
    let (sender, owned) = mpsc::channel();
    thread::spawn(move || {
        let _watcher = watcher;
        for change in owners {
            if change.args().unwrap().new_owner().is_some() {
                sender.send(()).ok();
            }
        }
    });
    owned
}

fn start_in(bus: &PrivateBus, args: &[&str], folders: &Folders) -> Running {
    let mut command = program(&bus.address, args);
    folders.apply(&mut command);
    Running(command.spawn().unwrap())
}

#[test]
fn the_default_for_folders_answers_show_in_folder() {
    let bus = PrivateBus::start();
    let folders = Folders::new("default-answers", true);
    let owned = file_manager_owned(&bus);

    let _service = start_in(&bus, &["--service"], &folders);

    owned
        .recv_timeout(WAIT)
        .expect("the default takes the file manager's name");
}

#[test]
fn a_service_that_isnt_the_default_leaves_show_in_folder_alone() {
    let bus = PrivateBus::start();
    let folders = Folders::new("not-default", false);
    let owned = file_manager_owned(&bus);
    let names = client(&bus);
    let app_owned = DBusProxy::new(&names)
        .unwrap()
        .receive_name_owner_changed_with_args(&[(0, APP_ID)])
        .unwrap();
    let (sender, started) = mpsc::channel();
    thread::spawn(move || {
        for _ in app_owned {
            sender.send(()).ok();
        }
    });

    let _service = start_in(&bus, &["--service"], &folders);

    started
        .recv_timeout(WAIT)
        .expect("the service takes its name");
    assert!(owned.recv_timeout(QUIET).is_err());
}

#[test]
fn started_for_show_in_folder_it_answers_without_being_the_default() {
    let bus = PrivateBus::start();
    let folders = Folders::new("activated", false);
    let owned = file_manager_owned(&bus);

    let _service = start_in(&bus, &["--file-manager"], &folders);

    owned
        .recv_timeout(WAIT)
        .expect("D-Bus started it for the name, so it takes it");
}

#[test]
fn started_for_show_in_folder_while_running_has_the_first_answer() {
    let bus = PrivateBus::start();
    let folders = Folders::new("activated-running", false);
    let core = nova_files::spawn().unwrap();
    let Started::Service(first) =
        nova_files_dbus::start(bus.bus(), core.connector, |_| {}).unwrap()
    else {
        panic!("the first start serves");
    };
    let owned = file_manager_owned(&bus);

    let mut second = program(&bus.address, &["--file-manager"]);
    folders.apply(&mut second);
    assert!(finish(second).status.success());

    owned
        .recv_timeout(WAIT)
        .expect("the running instance takes the name");
    drop(first);
}
