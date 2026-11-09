//! The public D-Bus interface that launchers and shells use to search and
//! open files.
//!
//! [`start`] claims the application's bus name. The first process to claim
//! it becomes the running instance and serves, on the application's object:
//! - the Nova Files interface (`files`, `Ping` for now),
//! - the search interface (`search`), versioned on its own,
//! - the standard `org.freedesktop.Application`, whose `Activate` and
//!   `Open` ask for a new window.
//!
//! [`Service::follow_desktop_look`] reads the desktop's light or dark
//! preference and accent from the settings portal for the theme.
//!
//! [`Service::serve_file_picker`] then serves the portal's `FileChooser`
//! interface at the portal path (`file_chooser`) and asks for the file
//! picker's portal bus name, waiting in the bus's queue if another program
//! holds it. A later process finds
//! the application's name taken and gets a [`RunningInstance`] to hand its
//! launch over to; it never serves the picker or asks for its name.
//!
//! Each module is one interface, so search and the picker grow in files of
//! their own. Our own interfaces are named `io.github.ImperaLuna.NovaFiles*`
//! and carry a version number: a change that breaks callers gets a new
//! interface beside the old one.
//!
//! The core stops 3 minutes after its last connection. A call answered at
//! once connects for the call (`Ping`); a request that outlives its call (a
//! search still sending stages, an open picker) takes a hold (`holds`),
//! which ends when the request does or when its caller leaves the bus.

#![forbid(unsafe_code)]

mod application;
mod desktop_look;
mod file_chooser;
mod files;
mod holds;
mod portal_options;
mod search;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use enumflags2::BitFlags;
use nova_files_api::{Connector, uri};
use zbus::blocking::connection::Builder;
use zbus::blocking::fdo::DBusProxy;
use zbus::blocking::{Connection, Proxy};
use zbus::fdo::{RequestNameFlags, RequestNameReply};
use zbus::zvariant::{ObjectPath, OwnedObjectPath, Value};

pub use application::INTERFACE as APPLICATION_INTERFACE;
pub use desktop_look::{
    ACCENT_COLOR, COLOR_SCHEME, INTERFACE as SETTINGS_INTERFACE, NAMESPACE as APPEARANCE_NAMESPACE,
};
pub use file_chooser::{
    BUS_NAME as PORTAL_BUS_NAME, INTERFACE as FILE_CHOOSER_INTERFACE,
    OBJECT_PATH as PORTAL_OBJECT_PATH, Picker,
};
pub use files::INTERFACE as FILES_INTERFACE;
pub use holds::{Hold, HoldError, Holds};
pub use nova_files_api::APP_ID;
pub use search::{
    ACTIVATION_TOKEN, INTERFACE as SEARCH_INTERFACE, OPEN_URI_INTERFACE, PORTAL_NAME, PORTAL_PATH,
};

/// The longest any D-Bus call made from here may take, and the longest
/// connecting to the bus may take.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(5);

const CONNECT_THREAD: &str = "nova-files-dbus-connect";
const NAME_WATCH_THREAD: &str = "nova-files-dbus-name-watch";
const BUS_WATCH_THREAD: &str = "nova-files-dbus-bus-watch";

/// Which bus to claim the name on.
#[derive(Clone)]
pub enum Bus {
    /// The user's session bus, as the program uses.
    Session,
    /// The bus at this address, such as a private one for tests.
    Address(String),
}

impl Bus {
    pub(crate) fn builder(&self) -> zbus::Result<Builder<'static>> {
        let builder = match self {
            Self::Session => Builder::session()?,
            Self::Address(address) => Builder::address(address.as_str())?,
        };
        Ok(builder.method_timeout(CALL_TIMEOUT))
    }
}

/// What [`start`] found on the bus.
pub enum Started {
    /// This process owns the bus name and serves the interfaces until the
    /// [`Service`] is dropped.
    Service(Service),
    /// Another process owns the name.
    AlreadyRunning(RunningInstance),
}

/// This process's hold on the bus names.
pub struct Service {
    connection: Connection,
    /// The bus `connection` is on, for the file picker's own connection.
    bus: Bus,
    portal: bool,
}

impl Service {
    /// Whether this process owned [`PORTAL_BUS_NAME`] when it began to
    /// serve the file picker. Another program may hold it; this process
    /// then waits in the bus's queue for it and becomes the file picker
    /// once it is freed.
    pub const fn serves_portal(&self) -> bool {
        self.portal
    }

    /// Serves the file picker, shown by `picker`, and asks for its bus
    /// name, waiting in the bus's queue when another program holds it. Pick
    /// requests hold the core `connector` reaches. The picker is optional,
    /// so any failure leaves the app running without it;
    /// [`Self::serves_portal`] says how it went.
    pub fn serve_file_picker(&mut self, connector: Connector, picker: impl Picker) {
        self.portal = Holds::new(connector, &self.bus)
            .is_ok_and(|holds| queue_for_portal(&self.connection, picker, holds));
    }

    /// Tells the core `connector` reaches what the desktop asks apps to
    /// look like, from the settings portal, and again each time that
    /// changes. Optional: on failure the theme stays Carbonfox's.
    ///
    /// # Errors
    /// When the portal's signal can't be listened to or the thread can't
    /// start.
    pub fn follow_desktop_look(&self, connector: Connector) -> zbus::Result<()> {
        desktop_look::follow(&self.connection, connector)
    }

    /// Runs `on_lost` once the connection to the bus closes, as when the
    /// bus daemon stops. Nothing can reach this process then, and the
    /// names it held are free for a new instance on the next bus.
    ///
    /// The watching thread stays blocked until then, holding a clone of the
    /// connection.
    ///
    /// # Errors
    /// When the watching thread can't start.
    pub fn on_bus_lost(&self, on_lost: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
        let connection = self.connection.clone();
        thread::Builder::new()
            .name(BUS_WATCH_THREAD.to_owned())
            .spawn(move || {
                zbus::block_on(connection.inner().closed());
                on_lost();
            })
            .map(drop)
    }

    /// Gives the bus names up and waits for the bus to confirm, so a
    /// process started after this returns can claim them at once, even
    /// while this connection is still open.
    ///
    /// # Errors
    /// When a call fails or takes longer than [`CALL_TIMEOUT`].
    pub fn release(&self) -> zbus::Result<()> {
        // Both, even when the first fails: the application name matters
        // most to a restart.
        let portal = self.connection.release_name(PORTAL_BUS_NAME);
        self.connection.release_name(APP_ID)?;
        portal.map(drop)
    }
}

/// The process that owns the bus name.
pub struct RunningInstance {
    connection: Connection,
    path: OwnedObjectPath,
}

impl RunningInstance {
    /// Asks the running instance for a new window.
    ///
    /// # Errors
    /// When the call fails or takes longer than [`CALL_TIMEOUT`].
    pub fn activate(&self) -> zbus::Result<()> {
        let proxy = Proxy::new(&self.connection, APP_ID, &self.path, APPLICATION_INTERFACE)?;
        let platform_data: HashMap<&str, Value<'_>> = HashMap::new();
        proxy.call::<_, _, ()>("Activate", &(platform_data,))
    }

    /// Asks the running instance for a new window on `folder`, which must
    /// be absolute.
    ///
    /// # Errors
    /// When the call fails or takes longer than [`CALL_TIMEOUT`].
    pub fn open(&self, folder: &Path) -> zbus::Result<()> {
        let proxy = Proxy::new(&self.connection, APP_ID, &self.path, APPLICATION_INTERFACE)?;
        let uris = vec![uri::file_uri(folder)];
        let platform_data: HashMap<&str, Value<'_>> = HashMap::new();
        proxy.call::<_, _, ()>("Open", &(uris, platform_data))
    }
}

/// Claims the bus name on `bus` and, if it was free, starts serving.
///
/// `connector` reaches the core for D-Bus calls the core answers.
/// `on_open` runs when another launch asks for a window, with the folder to
/// open it on (`None` for the home folder); it runs on a D-Bus thread and
/// must hand the request on rather than do the work.
///
/// # Errors
/// When the bus can't be reached within [`CALL_TIMEOUT`] or refuses the
/// connection.
pub fn start(
    bus: Bus,
    connector: Connector,
    on_open: impl Fn(Option<PathBuf>) + Send + Sync + 'static,
) -> zbus::Result<Started> {
    start_waiting(bus, connector, on_open, Duration::ZERO)
}

/// [`start`], but while the name is taken it asks again for up to `wait`
/// before settling for [`Started::AlreadyRunning`]. A restarted process
/// uses it in case the bus hasn't freed its old name yet.
///
/// # Errors
/// As [`start`].
pub fn start_waiting(
    bus: Bus,
    connector: Connector,
    on_open: impl Fn(Option<PathBuf>) + Send + Sync + 'static,
    wait: Duration,
) -> zbus::Result<Started> {
    let path = object_path()?;
    let connection = connect(bus.clone(), path.clone(), connector, on_open)?;
    // Watched before the first request, so a release in between still
    // counts.
    let freed = if wait.is_zero() {
        None
    } else {
        Some(watch_name_freed(&connection)?)
    };
    let deadline = Instant::now() + wait;
    loop {
        // Requested by hand, not with the builder's `name`, to pass
        // DoNotQueue: a second launch must learn the name is taken, not
        // wait in line for it.
        match connection.request_name_with_flags(APP_ID, RequestNameFlags::DoNotQueue.into()) {
            Ok(_) => {
                return Ok(Started::Service(Service {
                    connection,
                    bus,
                    portal: false,
                }));
            }
            Err(zbus::Error::NameTaken) => {
                let left = deadline.saturating_duration_since(Instant::now());
                let freed_in_time = freed
                    .as_ref()
                    .is_some_and(|freed| freed.recv_timeout(left).is_ok());
                if !freed_in_time {
                    // The same connection makes the call; the objects it
                    // also serves are never reached, since the name points
                    // at the other process.
                    return Ok(Started::AlreadyRunning(RunningInstance {
                        connection,
                        path,
                    }));
                }
            }
            Err(error) => return Err(error),
        }
    }
}

/// Serves the file picker and asks for its bus name; whether this process
/// owns it now.
fn queue_for_portal(connection: &Connection, picker: impl Picker, holds: Holds) -> bool {
    let served = connection.object_server().at(
        PORTAL_OBJECT_PATH,
        file_chooser::FileChooser::new(picker, holds),
    );
    if !matches!(served, Ok(true)) {
        return false;
    }
    // With no flags, the bus queues the request while the name is taken
    // and never lets another program replace us. zbus's default flags do
    // the opposite.
    let no_flags = BitFlags::<RequestNameFlags>::EMPTY;
    matches!(
        connection.request_name_with_flags(PORTAL_BUS_NAME, no_flags),
        Ok(RequestNameReply::PrimaryOwner)
    )
}

/// Sends on the returned channel each time the bus name loses its owner.
///
/// The bus has no "until" for signals, so the watching thread stays blocked
/// until the next change of owner; only a restarted process starts one.
fn watch_name_freed(connection: &Connection) -> zbus::Result<mpsc::Receiver<()>> {
    let changes =
        DBusProxy::new(connection)?.receive_name_owner_changed_with_args(&[(0, APP_ID)])?;
    let (sender, freed) = mpsc::channel();
    thread::Builder::new()
        .name(NAME_WATCH_THREAD.to_owned())
        .spawn(move || {
            for change in changes {
                let owner_gone = change.args().is_ok_and(|args| args.new_owner().is_none());
                if owner_gone && sender.send(()).is_err() {
                    return;
                }
            }
        })
        .map_err(|error| zbus::Error::Failure(error.to_string()))?;
    Ok(freed)
}

/// Connects and serves the interfaces, giving up after [`CALL_TIMEOUT`].
fn connect(
    bus: Bus,
    path: OwnedObjectPath,
    connector: Connector,
    on_open: impl Fn(Option<PathBuf>) + Send + Sync + 'static,
) -> zbus::Result<Connection> {
    let holds = Holds::new(connector.clone(), &bus)?;
    let on_open = Arc::new(on_open);
    let search_open = Arc::clone(&on_open);
    let searches = search::Searches::new(connector.clone(), holds, move |folder| {
        search_open(folder);
    });
    let application =
        application::Application::new(connector.clone(), move |folder| on_open(folder));
    connect_within_timeout(bus, move |builder| {
        builder
            .serve_at(path.clone(), files::Files::new(connector))?
            .serve_at(path.clone(), searches)?
            .serve_at(path, application)
    })
}

/// Connects to `bus` with what `serve` adds to the connection, giving up
/// after [`CALL_TIMEOUT`].
///
/// zbus has no timeout for connecting and authenticating, so that happens
/// on its own thread; if the bus never answers, the thread is left blocked
/// and the caller carries on without D-Bus.
pub(crate) fn connect_within_timeout(
    bus: Bus,
    serve: impl FnOnce(Builder<'static>) -> zbus::Result<Builder<'static>> + Send + 'static,
) -> zbus::Result<Connection> {
    let (result_sender, result) = mpsc::channel();
    thread::Builder::new()
        .name(CONNECT_THREAD.to_owned())
        .spawn(move || {
            let connection = bus.builder().and_then(serve).and_then(Builder::build);
            result_sender.send(connection).ok();
        })
        .map_err(|error| zbus::Error::Failure(error.to_string()))?;
    result
        .recv_timeout(CALL_TIMEOUT)
        .map_err(|_| zbus::Error::Failure("the D-Bus session bus did not answer".to_owned()))?
}

/// The object path the application id implies, as the freedesktop
/// application spec derives it: `.` becomes `/` and `-` becomes `_`, so
/// `io.github.Example.My-App` becomes `/io/github/Example/My_App`.
///
/// # Errors
/// When the application id doesn't make a valid object path.
pub fn object_path() -> zbus::Result<OwnedObjectPath> {
    object_path_for(APP_ID)
}

fn object_path_for(app_id: &str) -> zbus::Result<OwnedObjectPath> {
    let path = format!("/{}", app_id.replace('.', "/").replace('-', "_"));
    Ok(ObjectPath::try_from(path)?.into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn dashes_in_the_app_id_become_underscores() {
        assert_eq!(
            super::object_path_for("io.github.Example.My-App")
                .unwrap()
                .as_str(),
            "/io/github/Example/My_App"
        );
    }

    #[test]
    fn object_path_follows_the_app_id() {
        assert_eq!(
            super::object_path().unwrap().as_str(),
            "/io/github/ImperaLuna/NovaFiles"
        );
    }
}
