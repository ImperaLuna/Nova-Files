#![forbid(unsafe_code)]

mod application;
mod desktop_look;
mod drives;
mod file_chooser;
mod file_manager;
mod files;
mod holds;
mod portal_options;
mod search;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use enumflags2::BitFlags;
use nova_files_api::{Command, Connector, Event, MessageType, WindowRequest, uri};
use zbus::blocking::connection::Builder;
use zbus::blocking::fdo::DBusProxy;
use zbus::blocking::{Connection, Proxy};
use zbus::fdo::{RequestNameFlags, RequestNameReply};
use zbus::zvariant::{ObjectPath, OwnedObjectPath, Value};

pub use application::{INTERFACE as APPLICATION_INTERFACE, SERVE_FILE_MANAGER};
pub use desktop_look::{
    ACCENT_COLOR, COLOR_SCHEME, GNOME_ICON_THEME, GNOME_INTERFACE, INTERFACE as SETTINGS_INTERFACE,
    KDE_ICON_THEME, KDE_ICONS, NAMESPACE as APPEARANCE_NAMESPACE,
};
pub use drives::{ACTION_TIMEOUT, UDISKS, UDISKS_PATH, Udisks};
pub use file_chooser::{
    BUS_NAME as PORTAL_BUS_NAME, INTERFACE as FILE_CHOOSER_INTERFACE,
    OBJECT_PATH as PORTAL_OBJECT_PATH, Picker,
};
pub use file_manager::{
    BUS_NAME as FILE_MANAGER_BUS_NAME, INTERFACE as FILE_MANAGER_INTERFACE,
    MAX_URIS as FILE_MANAGER_MAX_URIS, OBJECT_PATH as FILE_MANAGER_OBJECT_PATH,
};
pub use files::INTERFACE as FILES_INTERFACE;
pub use holds::{Hold, HoldError, Holds};
pub use nova_files_api::APP_ID;
pub use search::{
    ACTIVATION_TOKEN, INTERFACE as SEARCH_INTERFACE, OPEN_URI_INTERFACE, PORTAL_NAME, PORTAL_PATH,
};

pub const CALL_TIMEOUT: Duration = Duration::from_secs(5);

const CONNECT_THREAD: &str = "nova-files-dbus-connect";
const NAME_WATCH_THREAD: &str = "nova-files-dbus-name-watch";
const BUS_WATCH_THREAD: &str = "nova-files-dbus-bus-watch";
const FILE_MANAGER_THREAD: &str = "nova-files-dbus-file-manager";

#[derive(Clone)]
pub enum Bus {
    Session,
    System,
    Address(String),
}

impl Bus {
    pub(crate) fn builder(&self) -> zbus::Result<Builder<'static>> {
        let builder = match self {
            Self::Session => Builder::session()?,
            Self::System => Builder::system()?,
            Self::Address(address) => Builder::address(address.as_str())?,
        };
        Ok(builder.method_timeout(CALL_TIMEOUT))
    }
}

pub enum Started {
    Service(Service),
    AlreadyRunning(RunningInstance),
}

pub struct Service {
    connection: Connection,
    bus: Bus,
    portal: bool,
}

impl Service {
    pub const fn serves_portal(&self) -> bool {
        self.portal
    }

    pub fn serve_file_picker(&mut self, connector: Connector, picker: impl Picker) {
        self.portal = Holds::new(connector, &self.bus)
            .is_ok_and(|holds| queue_for_portal(&self.connection, picker, holds));
    }

    pub fn serve_file_manager_when(
        &self,
        should: impl FnOnce() -> bool + Send + 'static,
    ) -> std::io::Result<mpsc::Receiver<bool>> {
        let connection = self.connection.clone();
        let (sender, owned) = mpsc::channel();
        thread::Builder::new()
            .name(FILE_MANAGER_THREAD.to_owned())
            .spawn(move || {
                let owns = should() && queue_for(&connection, FILE_MANAGER_BUS_NAME);
                sender.send(owns).ok();
            })?;
        Ok(owned)
    }

    pub fn follow_desktop_look(&self, connector: Connector) -> zbus::Result<()> {
        desktop_look::follow(&self.connection, connector)
    }

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

    pub fn release(&self) -> zbus::Result<()> {
        let app = self.connection.release_name(APP_ID);
        let portal = self.connection.release_name(PORTAL_BUS_NAME);
        let file_manager = self.connection.release_name(FILE_MANAGER_BUS_NAME);
        app.and(portal).and(file_manager).map(drop)
    }
}

pub struct RunningInstance {
    connection: Connection,
    path: OwnedObjectPath,
}

impl RunningInstance {
    pub fn activate(&self) -> zbus::Result<()> {
        let proxy = Proxy::new(&self.connection, APP_ID, &self.path, APPLICATION_INTERFACE)?;
        let platform_data: HashMap<&str, Value<'_>> = HashMap::new();
        proxy.call::<_, _, ()>("Activate", &(platform_data,))
    }

    pub fn open(&self, folder: &Path) -> zbus::Result<()> {
        let proxy = Proxy::new(&self.connection, APP_ID, &self.path, APPLICATION_INTERFACE)?;
        let uris = vec![uri::file_uri(folder)];
        let platform_data: HashMap<&str, Value<'_>> = HashMap::new();
        proxy.call::<_, _, ()>("Open", &(uris, platform_data))
    }

    pub fn serve_file_manager(&self) -> zbus::Result<()> {
        let proxy = Proxy::new(&self.connection, APP_ID, &self.path, APPLICATION_INTERFACE)?;
        let parameter: Vec<Value<'_>> = Vec::new();
        let platform_data: HashMap<&str, Value<'_>> = HashMap::new();
        proxy.call::<_, _, ()>(
            "ActivateAction",
            &(SERVE_FILE_MANAGER, parameter, platform_data),
        )
    }
}

pub fn sent_types() -> Vec<MessageType> {
    vec![
        MessageType::of::<Command>(),
        MessageType::of::<nova_files_look_api::Command>(),
    ]
}

pub fn taken_types() -> Vec<MessageType> {
    vec![MessageType::of::<Event>()]
}

pub fn find_running(bus: &Bus) -> zbus::Result<Option<RunningInstance>> {
    let connection = connect_within_timeout(bus.clone(), Ok)?;
    let owned = DBusProxy::new(&connection)?.name_has_owner(APP_ID.try_into()?)?;
    if !owned {
        return Ok(None);
    }
    Ok(Some(RunningInstance {
        connection,
        path: object_path()?,
    }))
}

pub fn start(
    bus: Bus,
    connector: Connector,
    on_open: impl Fn(WindowRequest) + Send + Sync + 'static,
) -> zbus::Result<Started> {
    start_waiting(bus, connector, on_open, Duration::ZERO)
}

pub fn start_waiting(
    bus: Bus,
    connector: Connector,
    on_open: impl Fn(WindowRequest) + Send + Sync + 'static,
    wait: Duration,
) -> zbus::Result<Started> {
    let path = object_path()?;
    let connection = connect(bus.clone(), path.clone(), connector, on_open)?;
    let freed = if wait.is_zero() {
        None
    } else {
        Some(watch_name_freed(&connection)?)
    };
    let deadline = Instant::now() + wait;
    loop {
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

fn queue_for_portal(connection: &Connection, picker: impl Picker, holds: Holds) -> bool {
    let served = connection.object_server().at(
        PORTAL_OBJECT_PATH,
        file_chooser::FileChooser::new(picker, holds),
    );
    matches!(served, Ok(true)) && queue_for(connection, PORTAL_BUS_NAME)
}

fn queue_for(connection: &Connection, name: &'static str) -> bool {
    // With no flags the bus queues the request and never hands the name to another program.
    // https://dbus.freedesktop.org/doc/dbus-specification.html#bus-messages-request-name
    let no_flags = BitFlags::<RequestNameFlags>::EMPTY;
    matches!(
        connection.request_name_with_flags(name, no_flags),
        Ok(RequestNameReply::PrimaryOwner)
    )
}

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

fn connect(
    bus: Bus,
    path: OwnedObjectPath,
    connector: Connector,
    on_open: impl Fn(WindowRequest) + Send + Sync + 'static,
) -> zbus::Result<Connection> {
    let holds = Holds::new(connector.clone(), &bus)?;
    let on_open = Arc::new(on_open);
    let search_open = Arc::clone(&on_open);
    let searches = search::Searches::new(connector.clone(), holds, move |request| {
        search_open(request);
    });
    let manager_open = Arc::clone(&on_open);
    let file_manager = file_manager::FileManager::new(connector.clone(), move |request| {
        manager_open(request);
    });
    let application =
        application::Application::new(connector.clone(), move |request| on_open(request));
    connect_within_timeout(bus, move |builder| {
        builder
            .serve_at(path.clone(), files::Files::new(connector))?
            .serve_at(path.clone(), searches)?
            .serve_at(path, application)?
            .serve_at(FILE_MANAGER_OBJECT_PATH, file_manager)
    })
}

// zbus has no timeout for connecting and authenticating.
// https://docs.rs/zbus/latest/zbus/connection/struct.Builder.html
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
        .map_err(|_| zbus::Error::Failure("the D-Bus bus did not answer".to_owned()))?
}

pub fn object_path() -> zbus::Result<OwnedObjectPath> {
    object_path_for(APP_ID)
}

// The object path comes from the app id as the Desktop Entry spec's D-Bus activation says.
// https://specifications.freedesktop.org/desktop-entry-spec/latest/dbus.html
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
