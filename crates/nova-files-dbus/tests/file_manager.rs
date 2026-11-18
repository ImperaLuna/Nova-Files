//! Against a private D-Bus daemon, as an app's "Show in folder" calls it:
//! `org.freedesktop.FileManager1` asks for a window on each folder, or on
//! each folder holding the items with them selected, and refuses URIs that
//! aren't local files. The service takes the bus name only when it
//! should and it is free, waits behind another file manager without
//! replacing it, and takes it when a second start (or `--make-default`,
//! which finds the running instance without starting one) asks it to.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use nova_files_api::{WindowRequest, uri};
use nova_files_dbus::{
    FILE_MANAGER_BUS_NAME, FILE_MANAGER_INTERFACE, FILE_MANAGER_MAX_URIS, FILE_MANAGER_OBJECT_PATH,
    Service, Started,
};
use zbus::blocking::{Connection, Proxy};

mod support;

use support::{PrivateBus, WAIT};

/// How long a test waits for something that should not happen.
const QUIET: Duration = Duration::from_millis(200);

/// The running instance, with each window it is asked for on `windows`.
struct Running {
    service: Service,
    windows: Receiver<WindowRequest>,
    _core: nova_files_core::RunningCore,
}

fn running(bus: &PrivateBus) -> Running {
    let core = nova_files_core::spawn().unwrap();
    let (sender, windows) = mpsc::channel();
    let started = nova_files_dbus::start(bus.bus(), core.connector.clone(), move |request| {
        sender.send(request).ok();
    })
    .unwrap();
    let Started::Service(service) = started else {
        panic!("the first start should serve");
    };
    Running {
        service,
        windows,
        _core: core,
    }
}

/// Asks for the file manager's name, as when Nova Files is the folder
/// default; whether it owns it.
fn serve(service: &Service) -> bool {
    service
        .serve_file_manager_when(|| true)
        .unwrap()
        .recv_timeout(WAIT)
        .unwrap()
}

/// A plain D-Bus client on `bus`, as an app would connect.
fn client(bus: &PrivateBus) -> Connection {
    zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .method_timeout(WAIT)
        .build()
        .unwrap()
}

/// Calls `method` on the file manager with `uris`, as an app would.
fn call(client: &Connection, method: &str, uris: &[String]) -> zbus::Result<()> {
    let proxy = Proxy::new(
        client,
        FILE_MANAGER_BUS_NAME,
        FILE_MANAGER_OBJECT_PATH,
        FILE_MANAGER_INTERFACE,
    )
    .unwrap();
    proxy.call::<_, _, ()>(method, &(uris, ""))
}

fn uris(paths: &[&str]) -> Vec<String> {
    paths
        .iter()
        .map(|path| uri::file_uri(Path::new(path)))
        .collect()
}

fn showing(folder: &str, names: &[&str]) -> WindowRequest {
    WindowRequest {
        folder: Some(PathBuf::from(folder)),
        select: names.iter().map(Into::into).collect(),
    }
}

#[test]
fn show_items_opens_each_holding_folder_with_its_items_selected() {
    let bus = PrivateBus::start();
    let running = running(&bus);
    assert!(serve(&running.service));
    let app = client(&bus);

    call(&app, "ShowItems", &uris(&["/a/x", "/b/y y", "/a/z"])).unwrap();

    assert_eq!(
        running.windows.recv_timeout(WAIT),
        Ok(showing("/a", &["x", "z"]))
    );
    assert_eq!(
        running.windows.recv_timeout(WAIT),
        Ok(showing("/b", &["y y"]))
    );
    assert!(running.windows.recv_timeout(QUIET).is_err());
}

#[test]
fn show_item_properties_shows_the_items() {
    let bus = PrivateBus::start();
    let running = running(&bus);
    assert!(serve(&running.service));

    call(&client(&bus), "ShowItemProperties", &uris(&["/a/x"])).unwrap();

    assert_eq!(
        running.windows.recv_timeout(WAIT),
        Ok(showing("/a", &["x"]))
    );
}

#[test]
fn show_folders_opens_a_window_on_each() {
    let bus = PrivateBus::start();
    let running = running(&bus);
    assert!(serve(&running.service));

    call(&client(&bus), "ShowFolders", &uris(&["/a", "/b"])).unwrap();

    assert_eq!(running.windows.recv_timeout(WAIT), Ok(showing("/a", &[])));
    assert_eq!(running.windows.recv_timeout(WAIT), Ok(showing("/b", &[])));
}

#[test]
fn nothing_opens_unless_every_uri_is_a_local_file() {
    let bus = PrivateBus::start();
    let running = running(&bus);
    assert!(serve(&running.service));
    let app = client(&bus);
    let mut mixed = uris(&["/a/x"]);
    mixed.push("https://example.com/x".to_owned());

    assert!(call(&app, "ShowItems", &mixed).is_err());
    assert!(call(&app, "ShowFolders", &mixed).is_err());
    assert!(call(&app, "ShowItems", &uris(&["/"])).is_err());
    let too_many = vec![uri::file_uri(Path::new("/a/x")); FILE_MANAGER_MAX_URIS + 1];
    assert!(call(&app, "ShowItems", &too_many).is_err());
    assert!(running.windows.recv_timeout(QUIET).is_err());
}

#[test]
fn as_many_uris_as_the_limit_still_show() {
    let bus = PrivateBus::start();
    let running = running(&bus);
    assert!(serve(&running.service));

    let most = vec![uri::file_uri(Path::new("/a/x")); FILE_MANAGER_MAX_URIS];
    call(&client(&bus), "ShowItems", &most).unwrap();

    assert_eq!(
        running.windows.recv_timeout(WAIT),
        Ok(showing("/a", &["x"]))
    );
}

#[test]
fn another_file_manager_keeps_the_name_until_it_lets_go() {
    let bus = PrivateBus::start();
    let other = client(&bus);
    other.request_name(FILE_MANAGER_BUS_NAME).unwrap();
    let running = running(&bus);

    assert!(!serve(&running.service));
    let owner = zbus::blocking::fdo::DBusProxy::new(&other)
        .unwrap()
        .get_name_owner(FILE_MANAGER_BUS_NAME.try_into().unwrap())
        .unwrap();
    assert_eq!(
        Some(owner.as_str()),
        other.unique_name().map(|name| name.as_str())
    );
}

#[test]
fn when_it_shouldnt_answer_apps_reach_no_one() {
    let bus = PrivateBus::start();
    let running = running(&bus);
    let owned = running.service.serve_file_manager_when(|| false).unwrap();

    assert_eq!(owned.recv_timeout(WAIT), Ok(false));
    assert!(call(&client(&bus), "ShowItems", &uris(&["/a/x"])).is_err());
    assert!(running.windows.recv_timeout(QUIET).is_err());
}

#[test]
fn a_second_start_has_the_running_instance_answer() {
    let bus = PrivateBus::start();
    let running = running(&bus);
    let core = nova_files_core::spawn().unwrap();
    let Started::AlreadyRunning(second) =
        nova_files_dbus::start(bus.bus(), core.connector, |_| {}).unwrap()
    else {
        panic!("the second start should find the first");
    };

    second.serve_file_manager().unwrap();

    call(&client(&bus), "ShowItems", &uris(&["/a/x"])).unwrap();
    assert_eq!(
        running.windows.recv_timeout(WAIT),
        Ok(showing("/a", &["x"]))
    );
}

#[test]
fn finding_the_running_instance_starts_none() {
    let bus = PrivateBus::start();
    assert!(nova_files_dbus::find_running(&bus.bus()).unwrap().is_none());

    let running = running(&bus);
    nova_files_dbus::find_running(&bus.bus())
        .unwrap()
        .expect("the instance is running")
        .serve_file_manager()
        .unwrap();

    call(&client(&bus), "ShowItems", &uris(&["/a/x"])).unwrap();
    assert_eq!(
        running.windows.recv_timeout(WAIT),
        Ok(showing("/a", &["x"]))
    );
}
