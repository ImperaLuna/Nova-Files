//! Against a private D-Bus daemon: the first start serves `Ping` through
//! the real core, and a second start finds it and hands over a window
//! request through `org.freedesktop.Application.Activate`. `Ping` fails
//! once the core has stopped. `Open` hands a folder over the same way. The
//! running instance serves search and the file picker, and owns the portal
//! name unless another program does. A hold keeps a real core with a short
//! idle time up, and ends when it is dropped or its caller leaves the bus.

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
use nova_files_dbus::{
    APP_ID, APPLICATION_INTERFACE, FILE_CHOOSER_INTERFACE, FILES_INTERFACE, HoldError, Holds,
    PORTAL_BUS_NAME, PORTAL_OBJECT_PATH, RunningInstance, SEARCH_INTERFACE, Started,
};
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

    // `first` still holds its connection, so only the release freed it.
    let again = nova_files_dbus::start(bus.bus(), core.connector, |_| {}).unwrap();
    assert!(matches!(again, Started::Service(_)));
    drop(first);
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

#[test]
fn the_running_instance_serves_search_and_the_file_picker() {
    let bus = PrivateBus::start();
    let core = nova_files_core::spawn().unwrap();
    let Started::Service(service) =
        nova_files_dbus::start(bus.bus(), core.connector.clone(), |_| {}).unwrap()
    else {
        panic!("the first start owns the name");
    };
    assert!(service.serves_portal());

    let app = introspect(
        &bus,
        APP_ID,
        nova_files_dbus::object_path().unwrap().as_str(),
    );
    assert!(app.contains(SEARCH_INTERFACE), "{app}");
    let portal = introspect(&bus, PORTAL_BUS_NAME, PORTAL_OBJECT_PATH);
    assert!(portal.contains(FILE_CHOOSER_INTERFACE), "{portal}");

    // A second start leaves the portal name with the first.
    drop(second(&bus, &core.connector));
    assert!(introspect(&bus, PORTAL_BUS_NAME, PORTAL_OBJECT_PATH).contains(FILE_CHOOSER_INTERFACE));
}

#[test]
fn a_release_frees_the_portal_name_too() {
    let bus = PrivateBus::start();
    let core = nova_files_core::spawn().unwrap();
    let Started::Service(first) =
        nova_files_dbus::start(bus.bus(), core.connector.clone(), |_| {}).unwrap()
    else {
        panic!("the first start owns the name");
    };
    first.release().unwrap();

    let Started::Service(again) =
        nova_files_dbus::start(bus.bus(), core.connector, |_| {}).unwrap()
    else {
        panic!("the name was released");
    };
    assert!(again.serves_portal());
    drop(first);
}

#[test]
fn the_app_runs_without_the_portal_name_when_another_program_holds_it() {
    let bus = PrivateBus::start();
    let other = client(&bus);
    other.request_name(PORTAL_BUS_NAME).unwrap();
    let core = nova_files_core::spawn().unwrap();

    let Started::Service(service) =
        nova_files_dbus::start(bus.bus(), core.connector, |_| {}).unwrap()
    else {
        panic!("the application name is free");
    };
    assert!(!service.serves_portal());
    // Releasing leaves the other program's name alone.
    service.release().unwrap();
    let owner = zbus::blocking::fdo::DBusProxy::new(&client(&bus))
        .unwrap()
        .get_name_owner(PORTAL_BUS_NAME.try_into().unwrap())
        .unwrap();
    assert_eq!(owner.as_str(), other.unique_name().unwrap().as_str());
}

/// The introspection XML of `path` on the service owning `name`.
fn introspect(bus: &PrivateBus, name: &str, path: &str) -> String {
    let client = client(bus);
    let proxy = Proxy::new(&client, name, path, "org.freedesktop.DBus.Introspectable").unwrap();
    proxy.call::<_, _, String>("Introspect", &()).unwrap()
}

// Holds.

/// The core's idle time in these tests.
const IDLE: Duration = Duration::from_millis(300);
/// Long enough that an unheld core would have stopped.
const PAST_IDLE: Duration = Duration::from_millis(900);

struct Setup {
    bus: PrivateBus,
    service: Connection,
    holds: Holds,
    /// Gets a message once the core has stopped.
    core_ended: Receiver<()>,
}

fn setup() -> Setup {
    let bus = PrivateBus::start();
    let core = nova_files_core::spawn_with_idle_exit(IDLE).unwrap();
    let service = client(&bus);
    let holds = Holds::new(core.connector, &service).unwrap();
    let (ended, core_ended) = mpsc::channel();
    let thread = core.thread;
    std::thread::spawn(move || {
        thread.join().ok();
        ended.send(()).ok();
    });
    Setup {
        bus,
        service,
        holds,
        core_ended,
    }
}

fn take(setup: &Setup, caller: &Connection) -> Result<nova_files_dbus::Hold, HoldError> {
    let name = caller.unique_name().unwrap().to_owned();
    zbus::block_on(setup.holds.take(setup.service.inner(), &name.as_ref()))
}

#[test]
fn a_hold_keeps_the_core_up_until_its_caller_leaves_the_bus() {
    let setup = setup();
    let caller = client(&setup.bus);
    let name = caller.unique_name().unwrap().to_owned();
    let hold = take(&setup, &caller).unwrap();

    assert!(setup.core_ended.recv_timeout(PAST_IDLE).is_err());
    assert_eq!(setup.holds.count(&name.as_ref()), 1);

    // The caller crashes: it never drops its hold, it just leaves the bus.
    drop(caller);

    setup.core_ended.recv_timeout(WAIT).unwrap();
    assert_eq!(setup.holds.count(&name.as_ref()), 0);
    drop(hold);
}

#[test]
fn a_dropped_hold_lets_the_core_stop() {
    let setup = setup();
    let caller = client(&setup.bus);
    let first = take(&setup, &caller).unwrap();
    let second = take(&setup, &caller).unwrap();
    drop(first);

    assert!(setup.core_ended.recv_timeout(PAST_IDLE).is_err());

    drop(second);
    setup.core_ended.recv_timeout(WAIT).unwrap();
    drop(caller);
}

#[test]
fn a_caller_already_gone_gets_no_hold() {
    let setup = setup();
    let caller = client(&setup.bus);
    let name = caller.unique_name().unwrap().to_owned();
    drop(caller);

    let taken = zbus::block_on(setup.holds.take(setup.service.inner(), &name.as_ref()));

    let Err(error) = taken else {
        panic!("a caller that left gets no hold");
    };
    assert!(matches!(error, HoldError::CallerGone));
    assert_eq!(error.to_string(), "the caller has left the bus");
    assert_eq!(setup.holds.count(&name.as_ref()), 0);
    setup.core_ended.recv_timeout(WAIT).unwrap();
}

#[test]
fn no_hold_once_the_core_has_stopped() {
    let setup = setup();
    setup.core_ended.recv_timeout(WAIT).unwrap();
    let caller = client(&setup.bus);

    assert!(matches!(take(&setup, &caller), Err(HoldError::CoreGone)));
}
