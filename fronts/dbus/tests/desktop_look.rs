#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test setup should fail loudly"
)]

use std::sync::mpsc::Receiver;

use nova_files_api::{Color, Command, Connector, CoreMessage, DesktopLook, Mode};
use nova_files_dbus::{
    ACCENT_COLOR, APPEARANCE_NAMESPACE, COLOR_SCHEME, GNOME_ICON_THEME, GNOME_INTERFACE,
    KDE_ICON_THEME, KDE_ICONS, PORTAL_NAME, PORTAL_PATH, SETTINGS_INTERFACE, Service, Started,
};
use zbus::blocking::Connection;
use zbus::blocking::connection::Builder;
use zbus::fdo;
use zbus::zvariant::{OwnedValue, Structure, Value};

mod support;

use support::{PrivateBus, WAIT};

const PREFERS_LIGHT: u32 = 2;
const PREFERS_DARK: u32 = 1;
const STARTING_SCHEME: u32 = PREFERS_LIGHT;
const ACCENT: (f64, f64, f64) = (1.0, 0.5, 0.0);
const ACCENT_RGB: Color = Color(0xFF_80_00);
const GNOME_THEME: &str = "Papirus-Dark";
const KDE_THEME: &str = "breeze";

struct Portal {
    scheme: u32,
}

#[zbus::interface(name = "org.freedesktop.portal.Settings")]
impl Portal {
    fn read_one(&self, namespace: &str, key: &str) -> fdo::Result<OwnedValue> {
        let value = match (namespace, key) {
            (APPEARANCE_NAMESPACE, COLOR_SCHEME) => Value::U32(self.scheme),
            (APPEARANCE_NAMESPACE, ACCENT_COLOR) => Value::Structure(Structure::from(ACCENT)),
            (GNOME_INTERFACE, GNOME_ICON_THEME) => Value::from(GNOME_THEME),
            (KDE_ICONS, KDE_ICON_THEME) => Value::from(KDE_THEME),
            _ => return Err(fdo::Error::Failed("not found".to_owned())),
        };
        Ok(value.try_to_owned().unwrap())
    }
}

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

struct BothMethods;

#[zbus::interface(name = "org.freedesktop.portal.Settings")]
impl BothMethods {
    #[expect(clippy::unused_self, reason = "the interface needs a receiver")]
    fn read_one(&self, namespace: &str, key: &str) -> fdo::Result<OwnedValue> {
        match (namespace, key) {
            (APPEARANCE_NAMESPACE, COLOR_SCHEME) => {
                Ok(Value::U32(PREFERS_LIGHT).try_to_owned().unwrap())
            }
            _ => Err(fdo::Error::Failed("not found".to_owned())),
        }
    }

    #[expect(clippy::unused_self, reason = "the interface needs a receiver")]
    fn read(&self, namespace: &str, key: &str) -> fdo::Result<OwnedValue> {
        match (namespace, key) {
            (APPEARANCE_NAMESPACE, ACCENT_COLOR) => {
                let wrapped = Value::Value(Box::new(Value::Structure(Structure::from(ACCENT))));
                Ok(wrapped.try_to_owned().unwrap())
            }
            _ => Err(fdo::Error::Failed("not found".to_owned())),
        }
    }
}

#[test]
fn a_key_the_new_method_lacks_is_not_asked_of_the_old_one() {
    let bus = PrivateBus::start();
    let _portal = serve_portal(&bus, BothMethods);
    let (connector, core) = nova_files_api::channel();
    let _service = follow(&bus, connector);
    let light = DesktopLook {
        mode: Some(Mode::Light),
        accent: None,
    };
    assert_eq!(next_look(&core), light);
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
    look_until(&core, light);

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

#[test]
fn the_icon_theme_reaches_the_core_and_follows_changes() {
    let bus = PrivateBus::start();
    let portal = serve_portal(
        &bus,
        Portal {
            scheme: STARTING_SCHEME,
        },
    );
    let (connector, core) = nova_files_api::channel();
    let _service = follow(&bus, connector);
    assert_eq!(next_icon_themes(&core), [GNOME_THEME, KDE_THEME]);

    changed(&portal, COLOR_SCHEME, Value::U32(PREFERS_DARK));
    changed_in(&portal, KDE_ICONS, KDE_ICON_THEME, Value::from("oxygen"));
    assert_eq!(next_icon_themes(&core), [GNOME_THEME, "oxygen"]);
    changed_in(&portal, GNOME_INTERFACE, GNOME_ICON_THEME, Value::from(""));
    assert_eq!(next_icon_themes(&core), ["oxygen"]);
}

fn next_icon_themes(core: &Receiver<CoreMessage>) -> Vec<String> {
    loop {
        if let Some(Command::DesktopIconThemes(names)) =
            core.recv_timeout(WAIT).unwrap().into_command::<Command>()
        {
            return names;
        }
    }
}

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

fn look_until(core: &Receiver<CoreMessage>, wanted: DesktopLook) {
    while next_look(core) != wanted {}
}

fn next_look(core: &Receiver<CoreMessage>) -> DesktopLook {
    loop {
        if let Some(Command::DesktopLook(look)) =
            core.recv_timeout(WAIT).unwrap().into_command::<Command>()
        {
            return look;
        }
    }
}
