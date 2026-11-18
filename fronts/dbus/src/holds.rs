use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use nova_files_api::{ClientGone, Command, Connection, Connector, CoreGone, Envelope, EventSink};
use zbus::blocking::fdo::DBusProxy;

use crate::Bus;
use zbus::names::{BusName, UniqueName};

const WATCH_THREAD: &str = "nova-files-dbus-holds";

type Held = HashMap<String, HashMap<u64, Connection>>;

#[derive(Clone)]
pub struct Holds {
    connector: Connector,
    held: Arc<Mutex<Held>>,
    next: Arc<AtomicU64>,
    bus: zbus::fdo::DBusProxy<'static>,
}

#[derive(Debug)]
pub enum HoldError {
    CoreGone,
    CallerGone,
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
    pub fn new(connector: Connector, bus: &Bus) -> zbus::Result<Self> {
        let connection = crate::connect_within_timeout(bus.clone(), Ok)?;
        let changes = DBusProxy::new(&connection)?.receive_name_owner_changed()?;
        let proxy = zbus::block_on(zbus::fdo::DBusProxy::new(connection.inner()))?;
        let holds = Self {
            connector,
            held: Arc::default(),
            next: Arc::default(),
            bus: proxy,
        };
        let held = Arc::downgrade(&holds.held);
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
                if let Some(held) = held.upgrade() {
                    lock(&held).clear();
                }
            })
            .map_err(|error| zbus::Error::Failure(error.to_string()))?;
        Ok(holds)
    }

    pub async fn take(&self, caller: &UniqueName<'_>) -> Result<Hold, HoldError> {
        self.take_with(caller, Discard).await
    }

    pub async fn take_with(
        &self,
        caller: &UniqueName<'_>,
        events: impl EventSink,
    ) -> Result<Hold, HoldError> {
        let connection = self
            .connector
            .connect(events)
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

    pub fn count(&self, caller: &UniqueName<'_>) -> usize {
        lock(&self.held)
            .get(caller.as_str())
            .map_or(0, HashMap::len)
    }
}

pub struct Hold {
    caller: String,
    id: u64,
    held: Arc<Mutex<Held>>,
}

impl Hold {
    pub fn send(&self, command: Command) -> Result<(), HoldError> {
        lock(&self.held)
            .get(&self.caller)
            .and_then(|holds| holds.get(&self.id))
            .ok_or(HoldError::CallerGone)?
            .send(command)
            .map_err(|CoreGone| HoldError::CoreGone)
    }
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

struct Discard;

impl EventSink for Discard {
    fn deliver_envelope(&self, _envelope: Envelope) -> Result<(), ClientGone> {
        Ok(())
    }
}

fn lock(held: &Mutex<Held>) -> MutexGuard<'_, Held> {
    held.lock().unwrap_or_else(PoisonError::into_inner)
}
