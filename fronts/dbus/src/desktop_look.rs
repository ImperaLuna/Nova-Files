use std::sync::mpsc;
use std::thread;

use futures::{Stream, StreamExt as _, future, stream};
use nova_files_api::{Color, Connector, CoreGone, DesktopLook, Event, Mode};
use nova_files_look_api::Command;
use zbus::blocking::Connection;
use zbus::fdo::DBusProxy;
use zbus::zvariant::Str;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{Message, Proxy};

use crate::search::{PORTAL_NAME, PORTAL_PATH};

pub const INTERFACE: &str = "org.freedesktop.portal.Settings";
pub const NAMESPACE: &str = "org.freedesktop.appearance";
pub const COLOR_SCHEME: &str = "color-scheme";
pub const ACCENT_COLOR: &str = "accent-color";
pub const GNOME_INTERFACE: &str = "org.gnome.desktop.interface";
pub const GNOME_ICON_THEME: &str = "icon-theme";
pub const KDE_ICONS: &str = "org.kde.kdeglobals.Icons";
pub const KDE_ICON_THEME: &str = "Theme";

const CHANGED: &str = "SettingChanged";
const READ_ONE: &str = "ReadOne";
const READ: &str = "Read";
const UNKNOWN_METHOD: &str = "org.freedesktop.DBus.Error.UnknownMethod";
const NOT_FOUND: &str = "org.freedesktop.portal.Error.NotFound";
const THREAD: &str = "nova-files-dbus-desktop-look";

const PREFERS_DARK: u32 = 1;
const PREFERS_LIGHT: u32 = 2;
const CHANNEL_MAX: f64 = 255.0;

pub fn follow(connection: &Connection, connector: Connector) -> zbus::Result<()> {
    let connection = connection.inner().clone();
    let (portal, heard) = zbus::block_on(async {
        let portal = Proxy::new(&connection, PORTAL_NAME, PORTAL_PATH, INTERFACE).await?;
        let changes = portal.receive_signal(CHANGED).await?.map(Heard::Changed);
        let owners = DBusProxy::new(&connection)
            .await?
            .receive_name_owner_changed_with_args(&[(0, PORTAL_NAME)])
            .await?
            .filter_map(|change| {
                let started = change.args().is_ok_and(|args| args.new_owner().is_some());
                future::ready(started.then_some(Heard::Started))
            });
        Ok::<_, zbus::Error>((portal, stream::select(changes, owners)))
    })?;
    thread::Builder::new()
        .name(THREAD.to_owned())
        .spawn(move || zbus::block_on(watch(&portal, heard, &connector)))
        .map(drop)
        .map_err(|error| zbus::Error::Failure(error.to_string()))
}

enum Heard {
    Changed(Message),
    Started,
}

#[derive(Default)]
struct Told {
    look: DesktopLook,
    icons: IconThemes,
}

impl Told {
    fn now(&self) -> (DesktopLook, IconThemes) {
        (self.look, self.icons.clone())
    }

    fn update(
        &mut self,
        now: (DesktopLook, IconThemes),
        connector: &Connector,
    ) -> Result<(), CoreGone> {
        let (look, icons) = now;
        if look != self.look {
            self.look = look;
            tell(connector, look)?;
        }
        if icons != self.icons {
            tell_icons(connector, &icons)?;
        }
        self.icons = icons;
        Ok(())
    }
}

async fn watch(
    portal: &Proxy<'_>,
    mut heard: impl Stream<Item = Heard> + Unpin,
    connector: &Connector,
) {
    let mut told = Told::default();
    let mut started = true;
    loop {
        let now = if started {
            started = false;
            read_all(portal, &told).await
        } else {
            match heard.next().await {
                None => return,
                Some(Heard::Started) => {
                    started = true;
                    continue;
                }
                Some(Heard::Changed(message)) => {
                    let Ok((namespace, key, value)) =
                        message.body().deserialize::<(String, String, OwnedValue)>()
                    else {
                        continue;
                    };
                    changed(told.now(), &namespace, &key, &value)
                }
            }
        };
        if told.update(now, connector).is_err() {
            return;
        }
    }
}

fn changed(
    (look, mut icons): (DesktopLook, IconThemes),
    namespace: &str,
    key: &str,
    value: &OwnedValue,
) -> (DesktopLook, IconThemes) {
    if namespace == NAMESPACE {
        return (with(look, key, value), icons);
    }
    icons.set(namespace, key, value);
    (look, icons)
}

fn heard(
    (look, mut icons): (DesktopLook, IconThemes),
    namespace: &str,
    key: &str,
    setting: Setting,
) -> (DesktopLook, IconThemes) {
    match setting {
        Setting::Value(value) => changed((look, icons), namespace, key, &value),
        Setting::Unset if namespace == NAMESPACE => (without(look, key), icons),
        Setting::Unset => {
            icons.clear(namespace, key);
            (look, icons)
        }
        Setting::Failed => (look, icons),
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct IconThemes {
    gnome: Option<String>,
    kde: Option<String>,
}

impl IconThemes {
    fn named(&self) -> Vec<String> {
        self.gnome.iter().chain(&self.kde).cloned().collect()
    }

    fn slot(&mut self, namespace: &str, key: &str) -> Option<&mut Option<String>> {
        match (namespace, key) {
            (GNOME_INTERFACE, GNOME_ICON_THEME) => Some(&mut self.gnome),
            (KDE_ICONS, KDE_ICON_THEME) => Some(&mut self.kde),
            _ => None,
        }
    }

    fn set(&mut self, namespace: &str, key: &str, value: &OwnedValue) {
        if let Some(slot) = self.slot(namespace, key) {
            *slot = <&Str<'_>>::try_from(peel(value))
                .ok()
                .map(|name| name.as_str().trim().to_owned())
                .filter(|name| !name.is_empty());
        }
    }

    fn clear(&mut self, namespace: &str, key: &str) {
        if let Some(slot) = self.slot(namespace, key) {
            *slot = None;
        }
    }
}

const KEYS: [(&str, &str); 4] = [
    (NAMESPACE, COLOR_SCHEME),
    (NAMESPACE, ACCENT_COLOR),
    (GNOME_INTERFACE, GNOME_ICON_THEME),
    (KDE_ICONS, KDE_ICON_THEME),
];

enum Setting {
    Value(OwnedValue),
    Unset,
    Failed,
}

async fn read_all(portal: &Proxy<'_>, told: &Told) -> (DesktopLook, IconThemes) {
    let mut now = told.now();
    for (namespace, key) in KEYS {
        now = heard(now, namespace, key, read(portal, namespace, key).await);
    }
    now
}

async fn read(portal: &Proxy<'_>, namespace: &str, key: &str) -> Setting {
    let answer = match portal.call(READ_ONE, &(namespace, key)).await {
        Err(zbus::Error::MethodError(name, _, _)) if name.as_str() == UNKNOWN_METHOD => {
            portal.call(READ, &(namespace, key)).await
        }
        answer => answer,
    };
    setting(answer)
}

fn setting(answer: zbus::Result<OwnedValue>) -> Setting {
    match answer {
        Ok(value) => Setting::Value(value),
        Err(zbus::Error::MethodError(name, _, _)) if name.as_str() == NOT_FOUND => Setting::Unset,
        Err(_) => Setting::Failed,
    }
}

fn with(mut look: DesktopLook, key: &str, value: &OwnedValue) -> DesktopLook {
    let value = peel(value);
    match key {
        COLOR_SCHEME => look.mode = mode(value),
        ACCENT_COLOR => look.accent = accent(value),
        _ => {}
    }
    look
}

fn without(mut look: DesktopLook, key: &str) -> DesktopLook {
    match key {
        COLOR_SCHEME => look.mode = None,
        ACCENT_COLOR => look.accent = None,
        _ => {}
    }
    look
}

fn peel<'a>(value: &'a Value<'a>) -> &'a Value<'a> {
    match value {
        Value::Value(inner) => inner,
        other => other,
    }
}

const fn mode(value: &Value<'_>) -> Option<Mode> {
    match value {
        Value::U32(PREFERS_DARK) => Some(Mode::Dark),
        Value::U32(PREFERS_LIGHT) => Some(Mode::Light),
        _ => None,
    }
}

fn accent(value: &Value<'_>) -> Option<Color> {
    let (red, green, blue) = <(f64, f64, f64)>::try_from(value).ok()?;
    let red = channel(red)?;
    let green = channel(green)?;
    let blue = channel(blue)?;
    Some(Color(u32::from_be_bytes([0, red, green, blue])))
}

fn channel(value: f64) -> Option<u8> {
    if !(0.0..=1.0).contains(&value) {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "checked to be from 0 to 1, so it rounds to 0 to 255"
    )]
    Some((value * CHANNEL_MAX).round() as u8)
}

fn tell(connector: &Connector, look: DesktopLook) -> Result<(), CoreGone> {
    send(connector, Command::DesktopLook(look))
}

fn tell_icons(connector: &Connector, icons: &IconThemes) -> Result<(), CoreGone> {
    send(connector, Command::DesktopIconThemes(icons.named()))
}

fn send(connector: &Connector, command: Command) -> Result<(), CoreGone> {
    let (nobody, _) = mpsc::channel::<Event>();
    connector.connect(nobody)?.send(command)
}

#[cfg(test)]
mod tests {
    use zbus::zvariant::Structure;

    use super::*;

    fn owned(value: &Value<'_>) -> OwnedValue {
        value.try_to_owned().unwrap()
    }

    #[test]
    fn color_scheme_reads_as_a_mode_or_nothing() {
        let look = |value| with(DesktopLook::default(), COLOR_SCHEME, &owned(&value)).mode;
        assert_eq!(look(Value::U32(PREFERS_DARK)), Some(Mode::Dark));
        assert_eq!(look(Value::U32(PREFERS_LIGHT)), Some(Mode::Light));
        assert_eq!(look(Value::U32(0)), None);
        assert_eq!(look(Value::from("dark")), None);
        let wrapped = Value::Value(Box::new(Value::U32(PREFERS_LIGHT)));
        assert_eq!(look(wrapped), Some(Mode::Light));
    }

    #[test]
    fn accent_color_reads_as_a_color_or_nothing() {
        let look = |red, green, blue| {
            let value = Value::Structure(Structure::from((red, green, blue)));
            with(DesktopLook::default(), ACCENT_COLOR, &owned(&value)).accent
        };
        assert_eq!(look(1.0, 0.5, 0.0), Some(Color(0xFF_80_00)));
        assert_eq!(look(0.0, 0.0, 1.0), Some(Color(0x00_00_FF)));
        assert_eq!(look(-1.0, -1.0, -1.0), None);
        assert_eq!(look(0.5, 1.5, 0.5), None);
        assert_eq!(look(0.5, 0.5, f64::NAN), None);
    }

    #[test]
    fn icon_themes_are_named_gnomes_first() {
        let mut icons = IconThemes::default();
        let name = |text: &str| owned(&Value::from(text));
        icons.set(KDE_ICONS, KDE_ICON_THEME, &name("breeze"));
        assert_eq!(icons.named(), ["breeze"]);
        icons.set(GNOME_INTERFACE, GNOME_ICON_THEME, &name(" Papirus-Dark "));
        assert_eq!(icons.named(), ["Papirus-Dark", "breeze"]);
        let wrapped = owned(&Value::Value(Box::new(Value::from("Adwaita"))));
        icons.set(GNOME_INTERFACE, GNOME_ICON_THEME, &wrapped);
        assert_eq!(icons.named(), ["Adwaita", "breeze"]);
        icons.set(GNOME_INTERFACE, GNOME_ICON_THEME, &name(""));
        assert_eq!(icons.named(), ["breeze"]);
        icons.set(KDE_ICONS, KDE_ICON_THEME, &owned(&Value::U32(1)));
        assert_eq!(icons.named(), Vec::<String>::new());
        icons.set(GNOME_INTERFACE, "font-name", &name("Inter"));
        icons.set("org.example", GNOME_ICON_THEME, &name("x"));
        assert_eq!(icons, IconThemes::default());
    }

    #[test]
    fn a_portal_that_fails_to_answer_keeps_the_look_told_and_a_missing_key_clears_it() {
        let told = (
            DesktopLook {
                mode: Some(Mode::Dark),
                accent: Some(Color(1)),
            },
            IconThemes {
                gnome: Some("Papirus".to_owned()),
                kde: Some("breeze".to_owned()),
            },
        );
        let failed = || {
            setting(Err(zbus::Error::InputOutput(
                std::io::Error::other("timed out").into(),
            )))
        };
        let unset = || {
            let name = zbus::names::OwnedErrorName::try_from(NOT_FOUND).unwrap();
            setting(Err(zbus::Error::MethodError(
                name,
                None,
                Message::method_call("/", "Read")
                    .unwrap()
                    .build(&())
                    .unwrap(),
            )))
        };
        let after = |setting: fn() -> Setting| {
            KEYS.iter().fold(told.clone(), |now, &(namespace, key)| {
                heard(now, namespace, key, setting())
            })
        };

        assert_eq!(after(failed), told);
        assert_eq!(
            after(unset),
            (DesktopLook::default(), IconThemes::default())
        );
    }

    #[test]
    fn another_key_leaves_the_look() {
        let look = DesktopLook {
            mode: Some(Mode::Dark),
            accent: Some(Color(1)),
        };
        assert_eq!(with(look, "contrast", &owned(&Value::U32(1))), look);
    }
}
