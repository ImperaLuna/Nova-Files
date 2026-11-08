//! The search interface: what launchers call to find files by name, then
//! open one or show it in its folder.
//!
//! `Search` returns at once; its stages come as `Found` signals sent only to
//! the caller, names as typed first, letters in order and typos after,
//! until one has `done` set. The caller picks the id, so it knows the
//! signals before the reply arrives. A newer search from the same caller
//! replaces the older one, whose stages stop; `Cancel` stops one. A search
//! has a core connection of its own and a hold (see `holds`) until its
//! last stage. `Failed` says a search couldn't run.
//!
//! Paths travel as strings: a path that isn't UTF-8 is left out of the
//! results, since a launcher couldn't hand it back.
//!
//! `Open` opens a folder in a Nova Files window and a file with the user's
//! default app, through the desktop portal's `OpenFile`. `ShowInFolder`
//! opens a window on the folder that holds the path. Both take the
//! launcher's `platform_data`, as `org.freedesktop.Application.Open` does,
//! and pass its `activation-token` to the portal, so the compositor lets
//! the opened app take focus.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use nova_files_api::{
    ClientGone, Command, Connection, Connector, Event, EventSink, Hit, Matched, SearchFailed,
    SearchId,
};
use rustix::fs::{Mode, OFlags};
use zbus::fdo;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{Fd, OwnedValue, Value};

use crate::holds::{Hold, Holds};

/// The interface name. The `#[zbus::interface]` attribute needs the literal
/// too; a test checks the two match.
pub const INTERFACE: &str = "io.github.ImperaLuna.NovaFiles.Search1";

/// The desktop portal and its interface for opening files.
pub const PORTAL_NAME: &str = "org.freedesktop.portal.Desktop";
pub const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
pub const OPEN_URI_INTERFACE: &str = "org.freedesktop.portal.OpenURI";

/// The `platform_data` key `org.freedesktop.Application` uses for an XDG
/// activation token, and the portal's option for it.
pub const ACTIVATION_TOKEN: &str = "activation-token";
const PORTAL_ACTIVATION_TOKEN: &str = "activation_token";

const FOUND: &str = "Found";
const FAILED: &str = "Failed";

/// One running search: dropping it ends the core connection and the hold.
struct Search {
    _hold: Hold,
    core: Connection,
}

/// Each caller's running search, by its unique bus name, with its id.
struct Running<T> {
    searches: Mutex<HashMap<String, (u64, T)>>,
}

impl<T> Default for Running<T> {
    fn default() -> Self {
        Self {
            searches: Mutex::default(),
        }
    }
}

impl<T> Running<T> {
    /// Makes `search` the caller's running one, ending the one before, and
    /// hands it to `start` while no stage can be sent for it yet.
    fn put<R>(&self, caller: &str, id: u64, search: T, start: impl FnOnce(&T) -> R) -> R {
        start(
            &self
                .lock()
                .entry(caller.to_owned())
                .insert_entry((id, search))
                .get()
                .1,
        )
    }

    /// Whether `id` is the caller's running search.
    fn is_current(&self, caller: &str, id: u64) -> bool {
        self.lock()
            .get(caller)
            .is_some_and(|(running, _)| *running == id)
    }

    /// Ends the caller's search `id`, if it is still the running one.
    fn end(&self, caller: &str, id: u64) {
        let mut searches = self.lock();
        if searches
            .get(caller)
            .is_some_and(|(running, _)| *running == id)
        {
            searches.remove(caller);
        }
    }

    /// The map stays whole whatever panicked while holding it: each change
    /// is one insert or remove.
    fn lock(&self) -> MutexGuard<'_, HashMap<String, (u64, T)>> {
        self.searches.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub struct Searches<F> {
    connector: Connector,
    holds: Holds,
    running: Arc<Running<Search>>,
    on_open: F,
}

impl<F> Searches<F> {
    pub fn new(connector: Connector, holds: Holds, on_open: F) -> Self {
        Self {
            connector,
            holds,
            running: Arc::default(),
            on_open,
        }
    }
}

#[zbus::interface(name = "io.github.ImperaLuna.NovaFiles.Search1")]
impl<F: Fn(Option<PathBuf>) + Send + Sync + 'static> Searches<F> {
    /// Starts search `id` for `query`, one word or several, with at most
    /// `limit` hits per stage. The stages arrive as `Found` signals.
    #[expect(
        clippy::too_many_arguments,
        reason = "three D-Bus arguments, plus the header and emitter zbus hands in"
    )]
    async fn search(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        id: u64,
        query: String,
        limit: u32,
    ) -> fdo::Result<()> {
        let caller = header
            .sender()
            .ok_or_else(|| fdo::Error::Failed("the call has no sender".to_owned()))?;
        let hold = self.holds.take(caller).await.map_err(failed)?;
        let stages = Stages {
            bus: zbus::blocking::Connection::from(emitter.connection().clone()),
            path: emitter.path().to_string(),
            caller: caller.to_string(),
            id,
            running: Arc::clone(&self.running),
        };
        let core = self.connector.connect(stages).map_err(failed)?;
        let command = Command::Search {
            id: SearchId(id),
            query,
            limit,
        };
        let search = Search { _hold: hold, core };
        // In first, so its stages count as current when they come.
        self.running
            .put(caller.as_str(), id, search, |search| {
                search.core.send(command)
            })
            .map_err(failed)
    }

    /// Stops search `id`, if it is the caller's running one.
    fn cancel(&self, #[zbus(header)] header: Header<'_>, id: u64) {
        let caller = header.sender().map(ToString::to_string);
        drop(header);
        if let Some(caller) = caller {
            self.running.end(&caller, id);
        }
    }

    /// Opens `path`: a folder in a Nova Files window, a file with the
    /// default app.
    async fn open(
        &self,
        #[zbus(connection)] bus: &zbus::Connection,
        path: &str,
        platform_data: HashMap<String, OwnedValue>,
    ) -> fdo::Result<()> {
        let path = absolute(path)?;
        if path.is_dir() {
            (self.on_open)(Some(path));
            return Ok(());
        }
        open_file(bus, &path, token(&platform_data)).await
    }

    /// Opens a Nova Files window on the folder that holds `path`.
    fn show_in_folder(
        &self,
        path: &str,
        platform_data: HashMap<String, OwnedValue>,
    ) -> fdo::Result<()> {
        // The window can't take an activation token yet.
        drop(platform_data);
        let path = absolute(path)?;
        let folder = path
            .parent()
            .ok_or_else(|| fdo::Error::InvalidArgs("the root has no folder".to_owned()))?;
        (self.on_open)(Some(folder.to_owned()));
        Ok(())
    }

    /// One stage of search `id`: (path, how it matched) pairs, best first.
    /// `done` marks the last.
    #[zbus(signal)]
    async fn found(
        emitter: &SignalEmitter<'_>,
        id: u64,
        hits: Vec<(String, String)>,
        done: bool,
    ) -> zbus::Result<()>;

    /// Search `id` couldn't run: `no-index` without a home or cache
    /// folder, `index-failed` when home couldn't be indexed.
    #[zbus(signal)]
    async fn failed(emitter: &SignalEmitter<'_>, id: u64, reason: String) -> zbus::Result<()>;
}

/// Sends one search's stages to its caller.
struct Stages {
    bus: zbus::blocking::Connection,
    path: String,
    caller: String,
    id: u64,
    running: Arc<Running<Search>>,
}

impl EventSink for Stages {
    fn deliver(&self, event: Event) -> Result<(), ClientGone> {
        if !self.running.is_current(&self.caller, self.id) {
            return Err(ClientGone);
        }
        let sent = match event {
            Event::Found { hits, done, .. } => {
                let hits: Vec<(String, String)> = hits.iter().filter_map(wire).collect();
                let sent = self.bus.emit_signal(
                    Some(self.caller.as_str()),
                    self.path.as_str(),
                    INTERFACE,
                    FOUND,
                    &(self.id, hits, done),
                );
                if done {
                    self.running.end(&self.caller, self.id);
                }
                sent
            }
            Event::SearchFailed { reason, .. } => {
                self.running.end(&self.caller, self.id);
                self.bus.emit_signal(
                    Some(self.caller.as_str()),
                    self.path.as_str(),
                    INTERFACE,
                    FAILED,
                    &(self.id, reason_name(reason)),
                )
            }
            _ => Ok(()),
        };
        sent.map_err(|_| ClientGone)
    }
}

/// A hit as the signal carries it, or `None` for a path that isn't UTF-8.
fn wire(hit: &Hit) -> Option<(String, String)> {
    let path = hit.path.to_str()?.to_owned();
    Some((path, matched_name(hit.matched).to_owned()))
}

const fn matched_name(matched: Matched) -> &'static str {
    match matched {
        Matched::Exact => "exact",
        Matched::WordStart => "word-start",
        Matched::Contains => "contains",
        Matched::InOrder => "in-order",
        Matched::OneTypo => "one-typo",
    }
}

const fn reason_name(reason: SearchFailed) -> &'static str {
    match reason {
        SearchFailed::NoIndex => "no-index",
        SearchFailed::IndexFailed => "index-failed",
    }
}

fn absolute(path: &str) -> fdo::Result<PathBuf> {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(fdo::Error::InvalidArgs(format!(
            "not an absolute path: {}",
            path.display()
        )))
    }
}

/// The launcher's activation token, if it gave one.
fn token(platform_data: &HashMap<String, OwnedValue>) -> Option<String> {
    let value = platform_data.get(ACTIVATION_TOKEN)?;
    String::try_from(value.try_clone().ok()?).ok()
}

/// Asks the portal to open `path` with the user's default app.
async fn open_file(bus: &zbus::Connection, path: &Path, token: Option<String>) -> fdo::Result<()> {
    let fd = rustix::fs::open(path, OFlags::PATH.union(OFlags::CLOEXEC), Mode::empty())
        .map_err(|error| fdo::Error::InvalidArgs(format!("{}: {error}", path.display())))?;
    let mut options: HashMap<&str, Value<'_>> = HashMap::new();
    if let Some(token) = token {
        options.insert(PORTAL_ACTIVATION_TOKEN, Value::from(token));
    }
    let portal = zbus::Proxy::new(bus, PORTAL_NAME, PORTAL_PATH, OPEN_URI_INTERFACE).await?;
    // No parent window: the launcher has closed by now.
    let request: zbus::zvariant::OwnedObjectPath = portal
        .call("OpenFile", &("", Fd::from(&fd), options))
        .await?;
    drop(request);
    Ok(())
}

fn failed(error: impl std::fmt::Display) -> fdo::Error {
    fdo::Error::Failed(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use zbus::object_server::Interface as _;

    use super::{INTERFACE, Running, Searches};

    const CALLER: &str = ":1.7";
    const OTHER: &str = ":1.8";

    #[test]
    fn a_newer_search_replaces_the_callers_older_one() {
        let dropped = Cell::new(0);
        let running = Running::default();
        running.put(CALLER, 1, Dropped(&dropped), |_| ());
        running.put(OTHER, 1, Dropped(&dropped), |_| ());
        assert_eq!(
            running.put(CALLER, 2, Dropped(&dropped), |_| "started"),
            "started"
        );
        assert_eq!(dropped.get(), 1);
        assert!(!running.is_current(CALLER, 1));
        assert!(running.is_current(CALLER, 2));
        assert!(running.is_current(OTHER, 1));
    }

    #[test]
    fn only_the_running_search_ends() {
        let dropped = Cell::new(0);
        let running = Running::default();
        running.put(CALLER, 2, Dropped(&dropped), |_| ());
        running.end(CALLER, 1);
        running.end(OTHER, 2);
        assert!(running.is_current(CALLER, 2));
        assert_eq!(dropped.get(), 0);
        running.end(CALLER, 2);
        assert!(!running.is_current(CALLER, 2));
        assert_eq!(dropped.get(), 1);
    }

    /// Counts its drops.
    struct Dropped<'a>(&'a Cell<u32>);

    impl Drop for Dropped<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn interface_name_matches_the_constant() {
        assert_eq!(
            Searches::<fn(Option<std::path::PathBuf>)>::name().as_str(),
            INTERFACE
        );
    }
}
