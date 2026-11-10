//! Against a private D-Bus daemon with a stand-in settings portal: the
//! running instance reads the desktop's light or dark and accent at once,
//! and passes each later change on to the core.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test setup should fail loudly"
)]

use std::sync::mpsc::Receiver;

use nova_files_api::{Color, Command, Connector, CoreMessage, DesktopLook, Mode};
use nova_files_dbus::{
    ACCENT_COLOR, APPEARANCE_NAMESPACE, COLOR_SCHEME, PORTAL_NAME, PORTAL_PATH, SETTINGS_INTERFACE,
    Service, Started,
};
use zbus::blocking::Connection;
use zbus::blocking::connection::Builder;
use zbus::fdo;
use zbus::zvariant::{OwnedValue, Structure, Value};

mod support;

use support::{PrivateBus, WAIT};

const PREFERS_LIGHT: u32 = 2;
const PREFERS_DARK: u32 = 1;
/// What the portal says before any change.
const STARTING_SCHEME: u32 = PREFERS_LIGHT;
/// Orange, as the portal sends it.
const ACCENT: (f64, f64, f64) = (1.0, 0.5, 0.0);
const ACCENT_RGB: Color = Color(0xFF_80_00);

/// The portal's settings, as a desktop that prefers light with an orange
/// accent would answer.
struct Portal {
    scheme: u32,
}

#[zbus::interface(name = "org.freedesktop.portal.Settings")]
impl Portal {
    fn read_one(&self, namespace: &str, key: &str) -> fdo::Result<OwnedValue> {
        let value = match (namespace, key) {
            (APPEARANCE_NAMESPACE, COLOR_SCHEME) => Value::U32(self.scheme),
            (APPEARANCE_NAMESPACE, ACCENT_COLOR) => Value::Structure(Structure::from(ACCENT)),
            _ => return Err(fdo::Error::Failed("not found".to_owned())),
        };
        Ok(value.try_to_owned().unwrap())
    }
}

/// A version 1 portal: only `Read`, which wraps the value in one more
/// variant. It says dark and sets no accent.
struct OldPortal;

#[zbus::interface(name = "org.freedesktop.portal.Settings")]
impl OldPortal {
    #[expect(clippy::unused_self, reason = "the interface needs a receiver")]
    fn read(&self, namespace: &str, key: &str) -> fdo::Result<OwnedValue> {
        match (namespace, key) {
            (APPEARANCE_NAMESPACE, COLOR_SCHEME) => {
                let wrapped = Value::Value(Box::new(Value::U32(PREFERS_DARK)));
                Ok(wrapped.try_to_owned().unwrap())
            }
            _ => Err(fdo::Error::Failed("not found".to_owned())),
        }
    }
}

#[test]
fn a_version_1_portal_is_read_too() {
    let bus = PrivateBus::start();
    let _portal = serve_portal(&bus, OldPortal);
    let (connector, core) = nova_files_api::channel();
    let _service = follow(&bus, connector);
    let dark = DesktopLook {
        mode: Some(Mode::Dark),
        accent: None,
    };
    assert_eq!(next_look(&core), dark);
}

#[test]
fn a_portal_that_starts_later_or_again_is_read_then() {
    let bus = PrivateBus::start();
    let (connector, core) = nova_files_api::channel();
    let _service = follow(&bus, connector);
    let first = serve_portal(
        &bus,
        Portal {
            scheme: PREFERS_LIGHT,
        },
    );
    let light = DesktopLook {
        mode: Some(Mode::Light),
        accent: Some(ACCENT_RGB),
    };
    // The first read can race the portal starting and see only part of
    // it; the read the portal's start brings on sees all of it.
    look_until(&core, light);

    // Started again with another preference, and no signal about it.
    first.release_name(PORTAL_NAME).unwrap();
    drop(first);
    let _second = serve_portal(
        &bus,
        Portal {
            scheme: PREFERS_DARK,
        },
    );
    let dark = DesktopLook {
        mode: Some(Mode::Dark),
        ..light
    };
    look_until(&core, dark);
}

#[test]
fn the_portal_name_matches_the_constant() {
    assert_eq!(
        <Portal as zbus::object_server::Interface>::name().as_str(),
        SETTINGS_INTERFACE
    );
}

#[test]
fn the_desktop_look_reaches_the_core_and_follows_changes() {
    let bus = PrivateBus::start();
    let portal = serve_portal(
        &bus,
        Portal {
            scheme: STARTING_SCHEME,
        },
    );
    let (connector, core) = nova_files_api::channel();
    let _service = follow(&bus, connector);

    let light = DesktopLook {
        mode: Some(Mode::Light),
        accent: Some(ACCENT_RGB),
    };
    assert_eq!(next_look(&core), light);

    changed(&portal, COLOR_SCHEME, Value::U32(PREFERS_DARK));
    let dark = DesktopLook {
        mode: Some(Mode::Dark),
        ..light
    };
    assert_eq!(next_look(&core), dark);

    // Another namespace, or the same look again, sends nothing; the next
    // real change is the next look the core hears.
    changed_in(
        &portal,
        "org.example",
        COLOR_SCHEME,
        Value::U32(PREFERS_LIGHT),
    );
    changed(&portal, COLOR_SCHEME, Value::U32(PREFERS_DARK));
    changed(
        &portal,
        ACCENT_COLOR,
        Value::Structure(Structure::from((-1.0, -1.0, -1.0))),
    );
    assert_eq!(
        next_look(&core),
        DesktopLook {
            accent: None,
            ..dark
        }
    );
}

/// Serves `portal` under the portal's name on `bus`.
fn serve_portal(bus: &PrivateBus, portal: impl zbus::object_server::Interface) -> Connection {
    Builder::address(bus.address.as_str())
        .unwrap()
        .serve_at(PORTAL_PATH, portal)
        .unwrap()
        .name(PORTAL_NAME)
        .unwrap()
        .build()
        .unwrap()
}

/// Starts the running instance on `bus` and has it follow the desktop for
/// the core `connector` reaches.
fn follow(bus: &PrivateBus, connector: Connector) -> Service {
    let Started::Service(service) =
        nova_files_dbus::start(bus.bus(), connector.clone(), |_| {}).unwrap()
    else {
        unreachable!("the first start serves");
    };
    service.follow_desktop_look(connector).unwrap();
    service
}

fn changed(portal: &Connection, key: &str, value: Value<'_>) {
    changed_in(portal, APPEARANCE_NAMESPACE, key, value);
}

fn changed_in(portal: &Connection, namespace: &str, key: &str, value: Value<'_>) {
    portal
        .emit_signal(
            None::<zbus::names::BusName<'_>>,
            PORTAL_PATH,
            SETTINGS_INTERFACE,
            "SettingChanged",
            &(namespace, key, value),
        )
        .unwrap();
}

/// Waits for the core to hear `wanted`, skipping the looks before it.
fn look_until(core: &Receiver<CoreMessage>, wanted: DesktopLook) {
    while next_look(core) != wanted {}
}

/// The next look sent to the core; other messages are skipped.
fn next_look(core: &Receiver<CoreMessage>) -> DesktopLook {
    loop {
        if let CoreMessage::Command {
            command: Command::DesktopLook(look),
            ..
        } = core.recv_timeout(WAIT).unwrap()
        {
            return look;
        }
    }
}
