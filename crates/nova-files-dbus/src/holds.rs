//! Holds: how a D-Bus caller keeps the core up while it still needs it.
//!
//! The core stops 3 minutes after its last client leaves, and a D-Bus call
//! is not a client by itself. A request that outlives its call (a search
//! still sending stages, a live result subscription, an open file picker)
//! takes a [`Hold`]: a connection to the core, owned by the caller's unique
//! bus name. The hold ends when it is dropped (the request is done or
//! cancelled) or when its caller leaves the bus, so an app that crashes
//! mid-request never keeps the core up for good.
//!
//! [`Holds`] watches `NameOwnerChanged` before the first hold is taken, and
//! [`Holds::take`] checks the caller is still on the bus after registering,
//! so a caller that leaves in between is still caught. The watch has a bus
//! connection of its own, owning no names, so its thread (blocked until
//! the next signal) never keeps the app's names taken. If that connection
//! breaks, every hold is dropped and no new one is given: a hold nobody
//! watches could outlive its caller.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use nova_files_api::{ClientGone, Connection, Connector, CoreGone, Event, EventSink};
use zbus::blocking::fdo::DBusProxy;

use crate::Bus;
use zbus::names::{BusName, UniqueName};

const WATCH_THREAD: &str = "nova-files-dbus-holds";

/// Each caller's holds, by the hold's id.
type Held = HashMap<String, HashMap<u64, Connection>>;

/// The holds D-Bus callers have on the core. Cheap to clone; clones share
/// the holds.
#[derive(Clone)]
pub struct Holds {
    connector: Connector,
    held: Arc<Mutex<Held>>,
    next: Arc<AtomicU64>,
    /// Asks the bus whether a caller is still there, on the watch's
    /// connection: once the watch has ended, this fails too.
    bus: zbus::fdo::DBusProxy<'static>,
}

/// Why [`Holds::take`] gave no hold.
#[derive(Debug)]
pub enum HoldError {
    /// The core has stopped.
    CoreGone,
    /// The caller left the bus before the hold was taken.
    CallerGone,
    /// The bus didn't answer whether the caller is still there.
    Bus(zbus::Error),
}

impl std::fmt::Display for HoldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CoreGone => CoreGone.fmt(f),
            Self::CallerGone => f.write_str("the caller has left the bus"),
            Self::Bus(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for HoldError {}

impl Holds {
    /// Holds on the core `connector` reaches, dropped when their caller
    /// leaves `bus`.
    ///
    /// # Errors
    /// When the bus can't be reached, refuses the signal subscription, or
    /// the watching thread can't start.
    pub fn new(connector: Connector, bus: &Bus) -> zbus::Result<Self> {
        let connection = bus.builder()?.build()?;
        let changes = DBusProxy::new(&connection)?.receive_name_owner_changed()?;
        let proxy = zbus::block_on(zbus::fdo::DBusProxy::new(connection.inner()))?;
        let holds = Self {
            connector,
            held: Arc::default(),
            next: Arc::default(),
            bus: proxy,
        };
        let held = Arc::downgrade(&holds.held);
        // Blocked until the next change of owner; it ends at the first
        // change after the last clone of the holds is gone.
        thread::Builder::new()
            .name(WATCH_THREAD.to_owned())
            .spawn(move || {
                for change in changes {
                    let Some(held) = held.upgrade() else { return };
                    let Ok(args) = change.args() else { continue };
                    if args.new_owner().is_none() {
                        lock(&held).remove(args.name().as_str());
                    }
                }
                // The connection is gone, so callers leaving can't be seen.
                if let Some(held) = held.upgrade() {
                    lock(&held).clear();
                }
            })
            .map_err(|error| zbus::Error::Failure(error.to_string()))?;
        Ok(holds)
    }

    /// Takes a hold on the core for `caller`, a unique name on the bus.
    ///
    /// # Errors
    /// [`HoldError`] when the core has stopped, the caller has left, or
    /// the bus doesn't say (as once the watch has ended).
    pub async fn take(&self, caller: &UniqueName<'_>) -> Result<Hold, HoldError> {
        let connection = self
            .connector
            .connect(Discard)
            .map_err(|CoreGone| HoldError::CoreGone)?;
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        lock(&self.held)
            .entry(caller.to_string())
            .or_default()
            .insert(id, connection);
        let hold = Hold {
            caller: caller.to_string(),
            id,
            held: Arc::clone(&self.held),
        };
        // Registered first, so a caller leaving now is seen by the watch or
        // by this check.
        let present = self
            .bus
            .name_has_owner(BusName::Unique(caller.clone()))
            .await
            .map_err(|error| HoldError::Bus(error.into()))?;
        if present {
            Ok(hold)
        } else {
            Err(HoldError::CallerGone)
        }
    }

    /// How many holds `caller` has.
    pub fn count(&self, caller: &UniqueName<'_>) -> usize {
        lock(&self.held)
            .get(caller.as_str())
            .map_or(0, HashMap::len)
    }
}

/// One caller's hold on the core; dropping it lets the core go.
pub struct Hold {
    caller: String,
    id: u64,
    held: Arc<Mutex<Held>>,
}

impl Drop for Hold {
    fn drop(&mut self) {
        let mut held = lock(&self.held);
        if let Some(holds) = held.get_mut(&self.caller) {
            holds.remove(&self.id);
            if holds.is_empty() {
                held.remove(&self.caller);
            }
        }
    }
}

/// A hold only keeps the core up; the core sends it nothing.
struct Discard;

impl EventSink for Discard {
    fn deliver(&self, _event: Event) -> Result<(), ClientGone> {
        Ok(())
    }
}

/// The map stays whole whatever panicked while holding it: each change is
/// one insert or remove.
fn lock(held: &Mutex<Held>) -> MutexGuard<'_, Held> {
    held.lock().unwrap_or_else(PoisonError::into_inner)
}
