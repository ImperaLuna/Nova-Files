//! The desktop's look: whether it prefers dark or light, and its accent,
//! read from the freedesktop settings portal and handed to the core, which
//! uses them for the `System` appearance and as the primary color when the
//! theme file sets none.
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

const CHANGED: &str = "SettingChanged";
/// Since version 2 of the interface.
const READ_ONE: &str = "ReadOne";
/// Version 1's read, which wraps the value in one more variant.
const READ: &str = "Read";
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

/// Reads the look, then follows what `heard` brings until the core stops.
async fn watch(
    portal: &Proxy<'_>,
    mut heard: impl Stream<Item = Heard> + Unpin,
    connector: &Connector,
) {
    let mut look = DesktopLook::default();
    let mut started = true;
    loop {
        let changed = if started {
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
                    if namespace != NAMESPACE {
                        continue;
                    }
                    with(look, &key, &value)
                }
            }
        };
        // The core starts with the default, so it needs no word of it.
        if changed != look {
            look = changed;
            if tell(connector, look).is_err() {
                return;
            }
        }
    }
}

/// The look as the portal holds it now; nothing without a portal.
async fn read_all(portal: &Proxy<'_>) -> DesktopLook {
    let mut look = DesktopLook::default();
    for key in [COLOR_SCHEME, ACCENT_COLOR] {
        if let Some(value) = read(portal, key).await {
            look = with(look, key, &value);
        }
    }
    look
}

/// The portal's value for `key`; `None` without a portal.
async fn read(portal: &Proxy<'_>, key: &str) -> Option<OwnedValue> {
    match portal.call(READ_ONE, &(NAMESPACE, key)).await {
        Ok(value) => Some(value),
        Err(_) => portal.call(READ, &(NAMESPACE, key)).await.ok(),
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
    let (nobody, _) = mpsc::channel::<Event>();
    connector.connect(nobody)?.send(Command::DesktopLook(look))
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
    fn another_key_leaves_the_look() {
        let look = DesktopLook {
            mode: Some(Mode::Dark),
            accent: Some(Color(1)),
        };
        assert_eq!(with(look, "contrast", &owned(&Value::U32(1))), look);
    }
}
