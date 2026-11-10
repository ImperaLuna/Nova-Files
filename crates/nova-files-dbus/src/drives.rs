//! Watches the system's drives and volumes through udisks2, the mount
//! service on the system bus, for the core (see
//! [`nova_files_api::DriveService`]).
//!
//! Nothing happens until the core asks. Then a thread of its own connects
//! to the system bus and listens for udisks2's objects being added or
//! removed, for a drive's or block device's properties changing, and for
//! udisks2 starting or stopping, before it reads the objects the first
//! time, so no change slips in between. Each signal makes it read all the objects again
//! (`GetManagedObjects`, a few dozen objects, under a millisecond); the
//! signals already queued by then are taken together with it, so a burst,
//! such as a stick with several partitions being plugged in, costs one
//! read. The core passes a list on only when it differs from the last.
//! udisks2's own job objects report progress many times a second and say
//! nothing about drives, so the rules leave them out.
//!
//! Without a system bus or udisks2 the list is empty; udisks2 starting
//! later sends the full list, and the bus closing empties it. A read that
//! fails while udisks2 runs (busy with a slow disk) keeps the last list.

mod objects;

use std::sync::Arc;
use std::thread;

use futures::{FutureExt as _, Stream, StreamExt as _, stream};
use nova_files_api::{DriveList, DriveService};
use zbus::fdo::{DBusProxy, ObjectManagerProxy};
use zbus::message::Type;
use zbus::names::BusName;
use zbus::proxy::CacheProperties;
use zbus::{MatchRule, MessageStream};

use crate::{Bus, connect_within_timeout};

/// udisks2's bus name.
pub const UDISKS: &str = "org.freedesktop.UDisks2";
/// Where udisks2 keeps its objects, and serves its object manager.
pub const UDISKS_PATH: &str = "/org/freedesktop/UDisks2";
/// Where udisks2 keeps its block devices (disks, partitions, loops).
const BLOCK_DEVICES_PATH: &str = "/org/freedesktop/UDisks2/block_devices";
/// Where udisks2 keeps its drives.
const DRIVES_PATH: &str = "/org/freedesktop/UDisks2/drives";
const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const PROPERTIES_CHANGED: &str = "PropertiesChanged";

const THREAD: &str = "nova-files-dbus-drives";

/// Hands the core drives and volumes as udisks2 on `bus` reports them.
pub struct Udisks {
    bus: Bus,
}

impl Udisks {
    /// Watches udisks2 on `bus`: the system bus, or a private one in
    /// tests.
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
}

/// Reads the drives, then again after each change, until the bus closes.
async fn watch(connection: &zbus::Connection, seen: &(dyn Fn(DriveList) + Send + Sync)) {
    let Ok((udisks, bus, mut heard)) = listen(connection).await else {
        seen(DriveList::default());
        return;
    };
    let mut told = false;
    loop {
        match read(&udisks, &bus).await {
            Some(list) => seen(list),
            // Clients wait for a first list, so they get one.
            None if !told => seen(DriveList::default()),
            None => {}
        }
        told = true;
        if heard.next().await.is_none() {
            // The bus is gone, and with it the way to mount anything.
            seen(DriveList::default());
            return;
        }
        // Whatever else is already queued is covered by the next read.
        while heard.next().now_or_never() == Some(Some(())) {}
    }
}

/// Proxies for udisks2's objects and the bus, and every change to
/// udisks2's drives and volumes: objects added or removed, a drive's or
/// block device's properties changed, udisks2 starting or stopping.
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
    // No sender in the rules: udisks2's unique name changes when it
    // restarts. A signal from anyone else only costs a read. udisks2's
    // job objects, which report progress many times a second, are left
    // out.
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

/// The drives udisks2 holds now: none when udisks2 isn't running, and
/// `None` when it is but didn't answer (busy probing a slow disk), so the
/// last list stands rather than every drive vanishing for a moment.
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
