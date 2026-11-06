//! Against a private D-Bus daemon: the first start serves `Ping` through
//! the real core, and a second start finds it and hands over a window
//! request through `org.freedesktop.Application.Activate`. `Ping` fails
//! once the core has stopped.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test setup should fail loudly"
)]

use std::sync::mpsc;
use std::time::Duration;

use nova_files_dbus::{APP_ID, FILES_INTERFACE, Started};
use zbus::blocking::Proxy;

mod support;

use support::{PrivateBus, WAIT};

#[test]
fn first_start_answers_ping_through_the_core() {
    let bus = PrivateBus::start();
    let core = nova_files_core::spawn().unwrap();
    let started = nova_files_dbus::start(bus.bus(), core.connector, || {}).unwrap();
    assert!(matches!(started, Started::Service(_)));

    assert!(ping(&bus).is_ok());
}

#[test]
fn ping_fails_once_the_core_has_stopped() {
    let bus = PrivateBus::start();
    let core = nova_files_core::spawn_with_idle_exit(Duration::ZERO).unwrap();
    core.thread.join().unwrap();
    let started = nova_files_dbus::start(bus.bus(), core.connector, || {}).unwrap();
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
    let first = nova_files_dbus::start(bus.bus(), core.connector.clone(), move || {
        activations.send(()).ok();
    })
    .unwrap();
    assert!(matches!(first, Started::Service(_)));

    let second = nova_files_dbus::start(bus.bus(), core.connector, || {}).unwrap();
    let Started::AlreadyRunning(running) = second else {
        panic!("the second start should find the first");
    };
    running.activate().unwrap();

    assert_eq!(activated.recv_timeout(WAIT), Ok(()));
}

#[test]
fn name_is_free_again_after_the_service_drops() {
    let bus = PrivateBus::start();
    let core = nova_files_core::spawn().unwrap();
    let first = nova_files_dbus::start(bus.bus(), core.connector.clone(), || {}).unwrap();
    drop(first);

    let again = nova_files_dbus::start(bus.bus(), core.connector, || {}).unwrap();
    assert!(matches!(again, Started::Service(_)));
}
