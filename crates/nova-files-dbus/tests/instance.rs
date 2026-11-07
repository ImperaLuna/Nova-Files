//! Against a private D-Bus daemon: the first start serves `Ping` through
//! the real core, and a second start finds it and hands over a window
//! request through `org.freedesktop.Application.Activate`. `Ping` fails
//! once the core has stopped. `Open` hands a folder over the same way.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test setup should fail loudly"
)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use nova_files_dbus::{APP_ID, APPLICATION_INTERFACE, FILES_INTERFACE, Started};
use zbus::blocking::Proxy;

mod support;

use support::{PrivateBus, WAIT};

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

/// Calls `Ping` on the service at the app id, as a launcher would.
fn ping(bus: &PrivateBus) -> zbus::Result<()> {
    let client = zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .method_timeout(WAIT)
        .build()
        .unwrap();
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
    let (activations, activated) = mpsc::channel();
    let first = nova_files_dbus::start(bus.bus(), core.connector.clone(), move |folder| {
        activations.send(folder).ok();
    })
    .unwrap();
    assert!(matches!(first, Started::Service(_)));

    let second = nova_files_dbus::start(bus.bus(), core.connector, |_| {}).unwrap();
    let Started::AlreadyRunning(running) = second else {
        panic!("the second start should find the first");
    };
    running.activate().unwrap();

    assert_eq!(activated.recv_timeout(WAIT), Ok(None));
}

#[test]
fn second_start_hands_its_folder_to_the_first() {
    let bus = PrivateBus::start();
    let core = nova_files_core::spawn().unwrap();
    let (opened, received) = mpsc::channel();
    let _first = nova_files_dbus::start(bus.bus(), core.connector.clone(), move |folder| {
        opened.send(folder).ok();
    })
    .unwrap();

    let second = nova_files_dbus::start(bus.bus(), core.connector, |_| {}).unwrap();
    let Started::AlreadyRunning(running) = second else {
        panic!("the second start should find the first");
    };
    let folder = PathBuf::from("/tmp/a folder");
    running.open(&folder).unwrap();

    assert_eq!(received.recv_timeout(WAIT), Ok(Some(folder)));
}

#[test]
fn open_refuses_uris_that_are_not_local_files() {
    let bus = PrivateBus::start();
    let core = nova_files_core::spawn().unwrap();
    let (opened, received) = mpsc::channel();
    let _first = nova_files_dbus::start(bus.bus(), core.connector, move |folder| {
        opened.send(folder).ok();
    })
    .unwrap();

    let client = zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .method_timeout(WAIT)
        .build()
        .unwrap();
    let application = Proxy::new(
        &client,
        APP_ID,
        nova_files_dbus::object_path().unwrap(),
        APPLICATION_INTERFACE,
    )
    .unwrap();
    let platform_data: HashMap<&str, zbus::zvariant::Value<'_>> = HashMap::new();
    let uris = vec!["file:///tmp", "https://example.com"];

    let answer = application.call::<_, _, ()>("Open", &(uris, "", platform_data));

    assert!(
        matches!(&answer, Err(zbus::Error::MethodError(name, _, _)) if name.as_str() == "org.freedesktop.DBus.Error.InvalidArgs"),
        "{answer:?}"
    );
    assert!(received.recv_timeout(Duration::from_millis(200)).is_err());
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
