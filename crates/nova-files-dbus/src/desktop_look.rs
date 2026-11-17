//! The desktop's look: whether it prefers dark or light, its accent and
//! its icon theme, read from the freedesktop settings portal and handed to
//! the core, which uses them for the `System` appearance, as the primary
//! color when the theme file sets none, and for file icons. The icon
//! themes are GNOME's setting and KDE's, where the portal has them; the
//! core picks among them.
//!
//! The portal is read once, then followed through its `SettingChanged`
//! signal on a thread of its own, which stays blocked until the next
//! change. A portal that takes the bus name later, or again after a
//! restart, says nothing of what it holds, so it is read again then. A change that alters the look reaches the core on a connection
//! made for it, so following the desktop never keeps the core awake.
//! Without the portal, or with a value the portal spec doesn't define, the
//! look says nothing there and the core keeps Carbonfox.

use std::sync::mpsc;
use std::thread;

use futures::{Stream, StreamExt as _, future, stream};
use nova_files_api::{Color, Command, Connector, CoreGone, DesktopLook, Event, Mode};
use zbus::blocking::Connection;
use zbus::fdo::DBusProxy;
use zbus::zvariant::Str;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{Message, Proxy};

use crate::search::{PORTAL_NAME, PORTAL_PATH};

/// The portal's settings interface.
pub const INTERFACE: &str = "org.freedesktop.portal.Settings";
/// Where the portal keeps the look.
pub const NAMESPACE: &str = "org.freedesktop.appearance";
/// `u32`: 0 no preference, 1 dark, 2 light.
pub const COLOR_SCHEME: &str = "color-scheme";
/// `(ddd)`: red, green and blue from 0 to 1; anything outside means unset.
pub const ACCENT_COLOR: &str = "accent-color";
/// Where GNOME's settings keep the icon theme, as the portal passes them.
pub const GNOME_INTERFACE: &str = "org.gnome.desktop.interface";
/// `s`: the icon theme's name.
pub const GNOME_ICON_THEME: &str = "icon-theme";
/// Where KDE's portal keeps the `[Icons]` group of `kdeglobals`.
pub const KDE_ICONS: &str = "org.kde.kdeglobals.Icons";
/// `s`: the icon theme's name.
pub const KDE_ICON_THEME: &str = "Theme";

const CHANGED: &str = "SettingChanged";
/// Since version 2 of the interface.
const READ_ONE: &str = "ReadOne";
/// Version 1's read, which wraps the value in one more variant.
const READ: &str = "Read";
/// What a version 1 portal answers to `ReadOne`; any other error (a key
/// it doesn't have) is the answer.
const UNKNOWN_METHOD: &str = "org.freedesktop.DBus.Error.UnknownMethod";
const THREAD: &str = "nova-files-dbus-desktop-look";

const PREFERS_DARK: u32 = 1;
const PREFERS_LIGHT: u32 = 2;
/// The top of a color channel.
const CHANNEL_MAX: f64 = 255.0;

/// Reads the look on `connection`, tells the core `connector` reaches, and
/// keeps telling it each time the look changes.
///
/// # Errors
/// When the signals can't be listened to or the thread can't start.
pub fn follow(connection: &Connection, connector: Connector) -> zbus::Result<()> {
    let connection = connection.inner().clone();
    // Listened to before the first read, so no change slips in between.
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

/// What the watching thread hears from the bus.
enum Heard {
    /// The portal says a setting changed.
    Changed(Message),
    /// A portal took the name: started late, or started again, without
    /// saying what it holds.
    Started,
}

/// What the desktop asks for, as last told to the core.
#[derive(Default)]
struct Told {
    look: DesktopLook,
    icons: IconThemes,
}

impl Told {
    /// Tells the core what changed between this and `now`, and keeps
    /// `now`. The core starts with the defaults, so it needs no word of
    /// them.
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

/// Reads the look, then follows what `heard` brings until the core stops.
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
            read_all(portal).await
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
                    changed(&told, &namespace, &key, &value)
                }
            }
        };
        if told.update(now, connector).is_err() {
            return;
        }
    }
}

/// What the desktop asks for once `key` of `namespace` is `value`.
fn changed(
    told: &Told,
    namespace: &str,
    key: &str,
    value: &OwnedValue,
) -> (DesktopLook, IconThemes) {
    let mut icons = told.icons.clone();
    if namespace == NAMESPACE {
        return (with(told.look, key, value), icons);
    }
    icons.set(namespace, key, value);
    (told.look, icons)
}

/// The icon theme names the portal holds, GNOME's and KDE's.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct IconThemes {
    gnome: Option<String>,
    kde: Option<String>,
}

impl IconThemes {
    /// The themes the desktop names: GNOME's, then KDE's.
    fn named(&self) -> Vec<String> {
        self.gnome.iter().chain(&self.kde).cloned().collect()
    }

    /// Takes `value` if `key` of `namespace` names an icon theme; an empty
    /// or non-string value unsets it.
    fn set(&mut self, namespace: &str, key: &str, value: &OwnedValue) {
        let slot = match (namespace, key) {
            (GNOME_INTERFACE, GNOME_ICON_THEME) => &mut self.gnome,
            (KDE_ICONS, KDE_ICON_THEME) => &mut self.kde,
            _ => return,
        };
        *slot = <&Str<'_>>::try_from(peel(value))
            .ok()
            .map(|name| name.as_str().trim().to_owned())
            .filter(|name| !name.is_empty());
    }
}

/// The look and icon themes as the portal holds them now; nothing without
/// a portal.
async fn read_all(portal: &Proxy<'_>) -> (DesktopLook, IconThemes) {
    let mut look = DesktopLook::default();
    for key in [COLOR_SCHEME, ACCENT_COLOR] {
        if let Some(value) = read(portal, NAMESPACE, key).await {
            look = with(look, key, &value);
        }
    }
    let mut icons = IconThemes::default();
    for (namespace, key) in [
        (GNOME_INTERFACE, GNOME_ICON_THEME),
        (KDE_ICONS, KDE_ICON_THEME),
    ] {
        if let Some(value) = read(portal, namespace, key).await {
            icons.set(namespace, key, &value);
        }
    }
    (look, icons)
}

/// The portal's value for `key` of `namespace`; `None` without a portal
/// or without the key.
async fn read(portal: &Proxy<'_>, namespace: &str, key: &str) -> Option<OwnedValue> {
    match portal.call(READ_ONE, &(namespace, key)).await {
        Ok(value) => Some(value),
        Err(zbus::Error::MethodError(name, _, _)) if name.as_str() == UNKNOWN_METHOD => {
            portal.call(READ, &(namespace, key)).await.ok()
        }
        Err(_) => None,
    }
}

/// `look` with `key` set to `value`; a key that isn't the look's leaves
/// it as it was.
fn with(mut look: DesktopLook, key: &str, value: &OwnedValue) -> DesktopLook {
    let value = peel(value);
    match key {
        COLOR_SCHEME => look.mode = mode(value),
        ACCENT_COLOR => look.accent = accent(value),
        _ => {}
    }
    look
}

/// The value inside `value` when it is a variant, as version 1's read
/// sends.
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

/// A channel from 0 to 1 as 0 to 255; `None` outside.
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

/// Sends `look` to the core on a connection made for it; nothing answers.
fn tell(connector: &Connector, look: DesktopLook) -> Result<(), CoreGone> {
    send(connector, Command::DesktopLook(look))
}

/// Sends the icon themes `icons` name to the core.
fn tell_icons(connector: &Connector, icons: &IconThemes) -> Result<(), CoreGone> {
    send(connector, Command::DesktopIconThemes(icons.named()))
}

/// Sends `command` to the core on a connection made for it; nothing
/// answers.
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
        // Version 1 wraps it once more.
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
        // Version 1 wraps it once more.
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
    fn another_key_leaves_the_look() {
        let look = DesktopLook {
            mode: Some(Mode::Dark),
            accent: Some(Color(1)),
        };
        assert_eq!(with(look, "contrast", &owned(&Value::U32(1))), look);
    }
}
