//! Watches the system's drives and volumes through udisks2, the mount
//! service on the system bus, for the core (see
//! [`nova_files_api::DriveService`]).
//!
//! Nothing happens until the core asks. Then a thread of its own connects
//! to the system bus and listens for every signal under udisks2's objects
//! (objects added or removed, properties changed) and for udisks2 starting
//! or stopping, before it reads the objects the first time, so no change
//! slips in between. Each signal makes it read all the objects again
//! (`GetManagedObjects`, a few dozen objects, under a millisecond); the
//! signals already queued by then are taken together with it, so a burst,
//! such as a stick with several partitions being plugged in, costs one
//! read. The core passes a list on only when it differs from the last.
//! udisks2's own job objects report progress many times a second and say
//! nothing about drives, so their signals are skipped.
//!
//! Without a system bus or udisks2 the list is empty; udisks2 starting
//! later sends the full list.

mod objects;

use std::sync::Arc;
use std::thread;

use futures::{FutureExt as _, Stream, StreamExt as _, future, stream};
use nova_files_api::{DriveList, DriveService};
use zbus::fdo::{DBusProxy, ObjectManagerProxy};
use zbus::message::Type;
use zbus::proxy::CacheProperties;
use zbus::{MatchRule, MessageStream};

use crate::{Bus, connect_within_timeout};

/// udisks2's bus name.
pub const UDISKS: &str = "org.freedesktop.UDisks2";
/// Where udisks2 keeps its objects, and serves its object manager.
pub const UDISKS_PATH: &str = "/org/freedesktop/UDisks2";
/// Where udisks2 keeps its job objects.
const JOBS_PATH: &str = "/org/freedesktop/UDisks2/jobs/";

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
    let Ok(mut heard) = listen(connection).await else {
        seen(DriveList::default());
        return;
    };
    loop {
        seen(read(connection).await);
        if heard.next().await.is_none() {
            return;
        }
        // Whatever else is already queued is covered by the next read.
        while heard.next().now_or_never() == Some(Some(())) {}
    }
}

/// Every change to udisks2's objects, and udisks2 starting or stopping.
async fn listen(connection: &zbus::Connection) -> zbus::Result<impl Stream<Item = ()> + Unpin> {
    // No sender in the rule: udisks2's unique name changes when it
    // restarts. A signal from anyone else only costs a read.
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .path_namespace(UDISKS_PATH)?
        .build();
    let changes = MessageStream::for_match_rule(rule, connection, None)
        .await?
        .filter_map(|message| {
            let about_drives = message.is_ok_and(|message| {
                message
                    .header()
                    .path()
                    .is_some_and(|path| !path.as_str().starts_with(JOBS_PATH))
            });
            future::ready(about_drives.then_some(()))
        });
    let owners = DBusProxy::new(connection)
        .await?
        .receive_name_owner_changed_with_args(&[(0, UDISKS)])
        .await?
        .map(drop);
    Ok(stream::select(changes, owners))
}

/// The drives udisks2 holds now; none when it can't be reached.
async fn read(connection: &zbus::Connection) -> DriveList {
    let objects = async {
        ObjectManagerProxy::builder(connection)
            .destination(UDISKS)?
            .path(UDISKS_PATH)?
            .cache_properties(CacheProperties::No)
            .build()
            .await?
            .get_managed_objects()
            .await
            .map_err(zbus::Error::from)
    };
    objects.await.map_or_else(
        |_| DriveList::default(),
        |objects| objects::drive_list(&objects),
    )
}
