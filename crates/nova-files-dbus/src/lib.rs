//! The public D-Bus interface that launchers and shells use to search and
//! open files.
//!
//! [`start`] claims the application's bus name. The first process to claim
//! it becomes the running instance and serves two objects at the same
//! path: the Nova Files interface (`Ping` for now) and the standard
//! `org.freedesktop.Application`, whose `Activate` asks for a new window.
//! A later process finds the name taken and gets a [`RunningInstance`] to
//! hand its launch over to.

#![forbid(unsafe_code)]

mod application;
mod files;

use std::collections::HashMap;
use std::time::Duration;

use nova_files_api::Connector;
use zbus::blocking::{self, Connection, Proxy};
use zbus::fdo::RequestNameFlags;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, Value};

pub use nova_files_api::APP_ID;

/// The longest any D-Bus call made or answered here may take.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(5);

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
        let proxy = Proxy::new(&self.connection, APP_ID, &self.path, application::INTERFACE)?;
        let platform_data: HashMap<&str, Value<'_>> = HashMap::new();
        proxy.call::<_, _, ()>("Activate", &(platform_data,))
    }
}

/// Claims the bus name on `bus` and, if it was free, starts serving.
///
/// `connector` reaches the core for D-Bus calls the core answers.
/// `on_activate` runs when another launch asks for a window; it runs on a
/// D-Bus thread and must hand the request on rather than do the work.
///
/// # Errors
/// When the bus can't be reached or refuses the connection.
pub fn start(
    bus: &Bus,
    connector: Connector,
    on_activate: impl Fn() + Send + Sync + 'static,
) -> zbus::Result<Started> {
    let path = object_path()?;
    let connection = bus
        .builder()?
        .serve_at(&path, files::Files::new(connector))?
        .serve_at(&path, application::Application::new(on_activate))?
        .build()?;
    // Requested by hand, not with the builder's `name`, to pass DoNotQueue:
    // a second launch must learn the name is taken, not wait in line for it.
    match connection.request_name_with_flags(APP_ID, RequestNameFlags::DoNotQueue.into()) {
        Ok(_) => Ok(Started::Service(Service {
            _connection: connection,
        })),
        Err(zbus::Error::NameTaken) => {
            drop(connection);
            let connection = bus.builder()?.build()?;
            Ok(Started::AlreadyRunning(RunningInstance {
                connection,
                path,
            }))
        }
        Err(error) => Err(error),
    }
}

/// The object path the application id implies:
/// `io.github.Example.App` becomes `/io/github/Example/App`.
fn object_path() -> zbus::Result<OwnedObjectPath> {
    let path = format!("/{}", APP_ID.replace('.', "/"));
    Ok(ObjectPath::try_from(path)?.into())
}
