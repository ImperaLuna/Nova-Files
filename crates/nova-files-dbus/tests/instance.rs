//! Against a private D-Bus daemon: the first start serves `Ping` through
//! the real core, and a second start finds it and hands over a window
//! request through `org.freedesktop.Application.Activate`. `Ping` fails
//! once the core has stopped. `Open` hands a folder over the same way.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use nova_files_api::Connector;
use nova_files_dbus::{APP_ID, APPLICATION_INTERFACE, FILES_INTERFACE, RunningInstance, Started};
use zbus::blocking::{Connection, Proxy};

mod support;

use support::{PrivateBus, WAIT};

/// How long a test waits for something that should not happen.
const QUIET: Duration = Duration::from_millis(200);

#[test]
fn first_start_answers_ping_through_the_core() {
    let bus = PrivateBus::start();
    let core = nova_files_core::spawn().unwrap();
    let started = nova_files_dbus::start(bus.bus(), core.connector, |_| {}).unwrap();
    assert!(matches!(started, Started::Service(_)));

    assert!(ping(&bus).is_ok());
}

#[test]
fn ping_fails_once_the_core_has_stopped() {
    let bus = PrivateBus::start();
    let core = nova_files_core::spawn_with_idle_exit(Duration::ZERO).unwrap();
    core.thread.join().unwrap();
    let started = nova_files_dbus::start(bus.bus(), core.connector, |_| {}).unwrap();
    assert!(matches!(started, Started::Service(_)));

    assert!(ping(&bus).is_err());
}

#[test]
fn activate_fails_once_the_core_has_stopped() {
    let bus = PrivateBus::start();
    let core = nova_files_core::spawn_with_idle_exit(Duration::ZERO).unwrap();
    core.thread.join().unwrap();
    let (_first, requests) = serve(&bus, &core.connector);

    assert!(second(&bus, &core.connector).activate().is_err());
    assert!(requests.recv_timeout(QUIET).is_err());
}

/// Starts the first instance on `bus`; each window it is asked for arrives
/// on the receiver, with its folder.
fn serve(bus: &PrivateBus, connector: &Connector) -> (Started, Receiver<Option<PathBuf>>) {
    let (sender, requests) = mpsc::channel();
    let started = nova_files_dbus::start(bus.bus(), connector.clone(), move |folder| {
        sender.send(folder).ok();
    })
    .unwrap();
    (started, requests)
}

/// Starts a second instance, which finds the first.
fn second(bus: &PrivateBus, connector: &Connector) -> RunningInstance {
    let started = nova_files_dbus::start(bus.bus(), connector.clone(), |_| {}).unwrap();
    let Started::AlreadyRunning(running) = started else {
        panic!("the second start should find the first");
    };
    running
}

/// A plain D-Bus client on `bus`, as a launcher would connect.
fn client(bus: &PrivateBus) -> Connection {
    zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .method_timeout(WAIT)
        .build()
        .unwrap()
}

/// Calls `Ping` on the service at the app id, as a launcher would.
fn ping(bus: &PrivateBus) -> zbus::Result<()> {
    let client = client(bus);
    let files = Proxy::new(
        &client,
        APP_ID,
        nova_files_dbus::object_path().unwrap(),
        FILES_INTERFACE,
    )
    .unwrap();
    files.call::<_, _, ()>("Ping", &())
}

#[test]
fn second_start_hands_its_window_to_the_first() {
    let bus = PrivateBus::start();
    let core = nova_files_core::spawn().unwrap();
    let (first, requests) = serve(&bus, &core.connector);
    assert!(matches!(first, Started::Service(_)));

    second(&bus, &core.connector).activate().unwrap();

    assert_eq!(requests.recv_timeout(WAIT), Ok(None));
}

#[test]
fn second_start_hands_its_folder_to_the_first() {
    let bus = PrivateBus::start();
    let core = nova_files_core::spawn().unwrap();
    let (_first, requests) = serve(&bus, &core.connector);
    let folder = PathBuf::from("/tmp/a folder");

    second(&bus, &core.connector).open(&folder).unwrap();

    assert_eq!(requests.recv_timeout(WAIT), Ok(Some(folder)));
}

#[test]
fn open_refuses_uris_that_are_not_local_files() {
    let bus = PrivateBus::start();
    let core = nova_files_core::spawn().unwrap();
    let (_first, requests) = serve(&bus, &core.connector);
    let client = client(&bus);
    let application = Proxy::new(
        &client,
        APP_ID,
        nova_files_dbus::object_path().unwrap(),
        APPLICATION_INTERFACE,
    )
    .unwrap();
    let platform_data: HashMap<&str, zbus::zvariant::Value<'_>> = HashMap::new();
    let uris = vec!["file:///tmp", "https://example.com"];

    let answer = application.call::<_, _, ()>("Open", &(uris, platform_data));

    assert!(
        matches!(&answer, Err(zbus::Error::MethodError(name, _, _)) if name.as_str() == "org.freedesktop.DBus.Error.InvalidArgs"),
        "{answer:?}"
    );
    assert!(requests.recv_timeout(QUIET).is_err());
}

#[test]
fn name_is_free_again_after_the_service_drops() {
    let bus = PrivateBus::start();
    let core = nova_files_core::spawn().unwrap();
    let first = nova_files_dbus::start(bus.bus(), core.connector.clone(), |_| {}).unwrap();
    drop(first);

    let again = nova_files_dbus::start(bus.bus(), core.connector, |_| {}).unwrap();
    assert!(matches!(again, Started::Service(_)));
}

#[test]
fn a_released_name_is_free_at_once() {
    let bus = PrivateBus::start();
    let core = nova_files_core::spawn().unwrap();
    let Started::Service(first) =
        nova_files_dbus::start(bus.bus(), core.connector.clone(), |_| {}).unwrap()
    else {
        panic!("the first start owns the name");
    };
    first.release().unwrap();

    let again = nova_files_dbus::start(bus.bus(), core.connector, |_| {}).unwrap();
    assert!(matches!(again, Started::Service(_)));
}

#[test]
fn a_waiting_start_takes_the_name_once_it_is_freed() {
    let bus = PrivateBus::start();
    let core = nova_files_core::spawn().unwrap();
    let Started::Service(first) =
        nova_files_dbus::start(bus.bus(), core.connector.clone(), |_| {}).unwrap()
    else {
        panic!("the first start owns the name");
    };
    // Released a little after the second start first finds it taken.
    let (_hold, held) = mpsc::channel::<()>();
    let release = std::thread::spawn(move || {
        held.recv_timeout(QUIET).ok();
        first.release().unwrap();
    });

    let again = nova_files_dbus::start_waiting(bus.bus(), core.connector, |_| {}, WAIT).unwrap();
    assert!(matches!(again, Started::Service(_)));
    release.join().unwrap();
}

#[test]
fn a_waiting_start_gives_up_after_its_wait() {
    let bus = PrivateBus::start();
    let core = nova_files_core::spawn().unwrap();
    let _first = nova_files_dbus::start(bus.bus(), core.connector.clone(), |_| {}).unwrap();

    let again = nova_files_dbus::start_waiting(bus.bus(), core.connector, |_| {}, QUIET).unwrap();
    assert!(matches!(again, Started::AlreadyRunning(_)));
}
