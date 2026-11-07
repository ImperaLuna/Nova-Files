//! The public D-Bus interface that launchers and shells use to search and
//! open files.
//!
//! [`start`] claims the application's bus name. The first process to claim
//! it becomes the running instance and serves two interfaces on one
//! object: the Nova Files interface (`Ping` for now) and the standard
//! `org.freedesktop.Application`, whose `Activate` and `Open` ask for a new
//! window.
//! A later process finds the name taken and gets a [`RunningInstance`] to
//! hand its launch over to.

#![forbid(unsafe_code)]

mod application;
mod files;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use nova_files_api::{Connector, uri};
use zbus::blocking::{self, Connection, Proxy};
use zbus::fdo::RequestNameFlags;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, Value};

pub use application::INTERFACE as APPLICATION_INTERFACE;
pub use files::INTERFACE as FILES_INTERFACE;
pub use nova_files_api::APP_ID;

/// The longest any D-Bus call made from here may take, and the longest
/// connecting to the bus may take.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(5);

const CONNECT_THREAD: &str = "nova-files-dbus-connect";

/// Which bus to claim the name on.
pub enum Bus {
    /// The user's session bus, as the program uses.
    Session,
    /// The bus at this address, such as a private one for tests.
    Address(String),
}

impl Bus {
    fn builder(&self) -> zbus::Result<blocking::connection::Builder<'static>> {
        let builder = match self {
            Self::Session => blocking::connection::Builder::session()?,
            Self::Address(address) => blocking::connection::Builder::address(address.as_str())?,
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

/// This process's hold on the bus name.
pub struct Service {
    _connection: Connection,
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
    let path = object_path()?;
    let connection = connect(bus, path.clone(), connector, on_open)?;
    // Requested by hand, not with the builder's `name`, to pass DoNotQueue:
    // a second launch must learn the name is taken, not wait in line for it.
    match connection.request_name_with_flags(APP_ID, RequestNameFlags::DoNotQueue.into()) {
        Ok(_) => Ok(Started::Service(Service {
            _connection: connection,
        })),
        // The same connection makes the call; the objects it also serves
        // are never reached, since the name points at the other process.
        Err(zbus::Error::NameTaken) => Ok(Started::AlreadyRunning(RunningInstance {
            connection,
            path,
        })),
        Err(error) => Err(error),
    }
}

/// Connects and serves the interfaces, giving up after [`CALL_TIMEOUT`].
///
/// zbus has no timeout for connecting and authenticating, so that happens
/// on its own thread; if the bus never answers, the thread is left blocked
/// and the caller carries on without D-Bus.
fn connect(
    bus: Bus,
    path: OwnedObjectPath,
    connector: Connector,
    on_open: impl Fn(Option<PathBuf>) + Send + Sync + 'static,
) -> zbus::Result<Connection> {
    let (result_sender, result) = mpsc::channel();
    thread::Builder::new()
        .name(CONNECT_THREAD.to_owned())
        .spawn(move || {
            let connection = bus.builder().and_then(|builder| {
                builder
                    .serve_at(&path, files::Files::new(connector.clone()))?
                    .serve_at(&path, application::Application::new(connector, on_open))?
                    .build()
            });
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
