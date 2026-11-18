mod act;
mod objects;

pub use act::ACTION_TIMEOUT;

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use futures::{FutureExt as _, Stream, StreamExt as _, stream};

use nova_files_api::{DriveAction, DriveFailure, DriveList, DriveService};
use zbus::fdo::{DBusProxy, ObjectManagerProxy};
use zbus::message::Type;
use zbus::names::BusName;
use zbus::proxy::CacheProperties;
use zbus::{MatchRule, MessageStream};

use crate::{Bus, connect_within_timeout};

pub const UDISKS: &str = "org.freedesktop.UDisks2";
pub const UDISKS_PATH: &str = "/org/freedesktop/UDisks2";
const BLOCK_DEVICES_PATH: &str = "/org/freedesktop/UDisks2/block_devices";
const DRIVES_PATH: &str = "/org/freedesktop/UDisks2/drives";
const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const PROPERTIES_CHANGED: &str = "PropertiesChanged";

const THREAD: &str = "nova-files-dbus-drives";

pub struct Udisks {
    bus: Bus,
}

impl Udisks {
    pub const fn new(bus: Bus) -> Self {
        Self { bus }
    }
}

impl DriveService for Udisks {
    fn follow(&self, seen: Box<dyn Fn(DriveList) + Send + Sync>) {
        let bus = self.bus.clone();
        let seen: Arc<dyn Fn(DriveList) + Send + Sync> = seen.into();
        let watcher = Arc::clone(&seen);
        let spawned = thread::Builder::new()
            .name(THREAD.to_owned())
            .spawn(move || match connect_within_timeout(bus, Ok) {
                Ok(connection) => zbus::block_on(watch(connection.inner(), &*watcher)),
                Err(_) => watcher(DriveList::default()),
            });
        if spawned.is_err() {
            seen(DriveList::default());
        }
    }

    fn act(&self, action: &DriveAction) -> Result<Option<PathBuf>, DriveFailure> {
        act::act(self.bus.clone(), action)
    }
}

async fn watch(connection: &zbus::Connection, seen: &(dyn Fn(DriveList) + Send + Sync)) {
    let Ok((udisks, bus, mut heard)) = listen(connection).await else {
        seen(DriveList::default());
        return;
    };
    let mut told = false;
    loop {
        match read(&udisks, &bus).await {
            Some(list) => seen(list),
            None if !told => seen(DriveList::default()),
            None => {}
        }
        told = true;
        if heard.next().await.is_none() {
            seen(DriveList::default());
            return;
        }
        while heard.next().now_or_never() == Some(Some(())) {}
    }
}

async fn listen(
    connection: &zbus::Connection,
) -> zbus::Result<(
    ObjectManagerProxy<'static>,
    DBusProxy<'static>,
    impl Stream<Item = ()> + Unpin,
)> {
    let udisks = ObjectManagerProxy::builder(connection)
        .destination(UDISKS)?
        .path(UDISKS_PATH)?
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    let bus = DBusProxy::new(connection).await?;
    let added_or_removed = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(OBJECT_MANAGER)?
        .path(UDISKS_PATH)?
        .build();
    let mut rules = vec![added_or_removed];
    for objects in [BLOCK_DEVICES_PATH, DRIVES_PATH] {
        rules.push(
            MatchRule::builder()
                .msg_type(Type::Signal)
                .interface(PROPERTIES)?
                .member(PROPERTIES_CHANGED)?
                .path_namespace(objects)?
                .build(),
        );
    }
    let mut changes = Vec::new();
    for rule in rules {
        changes.push(
            MessageStream::for_match_rule(rule, connection, None)
                .await?
                .map(drop),
        );
    }
    let owners = bus
        .receive_name_owner_changed_with_args(&[(0, UDISKS)])
        .await?
        .map(drop);
    Ok((
        udisks,
        bus,
        stream::select(stream::select_all(changes), owners),
    ))
}

async fn read(udisks: &ObjectManagerProxy<'_>, bus: &DBusProxy<'_>) -> Option<DriveList> {
    if let Ok(objects) = udisks.get_managed_objects().await {
        return Some(objects::drive_list(&objects));
    }
    let name = BusName::try_from(UDISKS).ok()?;
    match bus.name_has_owner(name).await {
        Ok(false) => Some(DriveList::default()),
        Ok(true) | Err(_) => None,
    }
}
