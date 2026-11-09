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
//! default app. The core starts that app itself when it can (see
//! `Command::OpenFile`); when it says it can't, as in a sandbox or for a
//! type it finds no app for, the desktop portal's `OpenFile` does. `ShowInFolder` opens
//! a window on the folder that holds the path. Both take the launcher's
//! `platform_data`, as `org.freedesktop.Application.Open` does, and pass
//! its `activation-token` on, so the compositor lets the opened app take
//! focus.

use std::collections::HashMap;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use futures::channel::oneshot;

use nova_files_api::{
    ClientGone, Command, Connector, Event, EventSink, Hit, Matched, SearchFailed, SearchId,
};
use rustix::fs::{Mode, OFlags};
use zbus::fdo;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{Fd, OwnedValue, Value};

use crate::files::ask_core;
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

/// How long `Open` waits to learn what a path is: well inside a caller's
/// own call timeout, so the caller gets this error.
const FILE_SYSTEM_TIMEOUT: Duration = Duration::from_secs(2);
const LOOK_THREAD: &str = "nova-files-dbus-look";

const FOUND: &str = "Found";
const FAILED: &str = "Failed";

/// One running search: a hold whose core connection runs it. Dropping it,
/// or the caller leaving, ends the connection and so the search.
type Search = Hold;

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

    /// Runs `send` if `id` is the caller's running search, ending it after
    /// when `last`. The map stays locked meanwhile, so once `end` or a
    /// newer `put` returns, nothing more is sent for the older search.
    fn send_if_current<R>(
        &self,
        caller: &str,
        id: u64,
        last: bool,
        send: impl FnOnce() -> R,
    ) -> Option<R> {
        let mut searches = self.lock();
        if searches
            .get(caller)
            .is_none_or(|(running, _)| *running != id)
        {
            return None;
        }
        let result = send();
        if last {
            searches.remove(caller);
        }
        drop(searches);
        Some(result)
    }

    #[cfg(test)]
    fn is_current(&self, caller: &str, id: u64) -> bool {
        self.lock()
            .get(caller)
            .is_some_and(|(running, _)| *running == id)
    }

    /// Ends the caller's search `id`, if it is still the running one.
    fn end(&self, caller: &str, id: u64) {
        self.send_if_current(caller, id, true, || ());
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

impl<F: Fn(Option<PathBuf>)> Searches<F> {
    /// Opens a window on `folder`, first telling the core, so it doesn't
    /// reach its idle exit while the window starts.
    fn open_window(&self, folder: PathBuf) -> fdo::Result<()> {
        self.connector.activity().map_err(failed)?;
        (self.on_open)(Some(folder));
        Ok(())
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
        let stages = Stages {
            bus: zbus::blocking::Connection::from(emitter.connection().clone()),
            path: emitter.path().to_string(),
            caller: caller.to_string(),
            id,
            running: Arc::clone(&self.running),
        };
        let search = self.holds.take_with(caller, stages).await.map_err(failed)?;
        let command = Command::Search {
            id: SearchId(id),
            query,
            limit,
        };
        // In first, so its stages count as current when they come.
        self.running
            .put(caller.as_str(), id, search, |search| search.send(command))
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
        match look_at(path.clone()).await? {
            Target::Folder => self.open_window(path),
            Target::File(file) => {
                let token = token(&platform_data);
                let command = Command::OpenFile {
                    path,
                    activation: token.clone(),
                };
                // Only a sure "not opened" goes to the portal: past the
                // wait, the core may still start the app, and it would
                // open twice.
                match ask_core(&self.connector, command).await? {
                    Event::FileOpened => Ok(()),
                    Event::FileNotOpened => open_file(bus, file, token).await,
                    _ => Err(failed("the core gave an unexpected answer")),
                }
            }
        }
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
        self.open_window(folder.to_owned())
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
        let (caller, path, id) = (Some(self.caller.as_str()), self.path.as_str(), self.id);
        let sent = match event {
            Event::Found { hits, done, .. } => {
                let hits: Vec<(String, String)> = hits.iter().filter_map(wire).collect();
                self.running.send_if_current(&self.caller, id, done, || {
                    self.bus
                        .emit_signal(caller, path, INTERFACE, FOUND, &(id, hits, done))
                })
            }
            Event::SearchFailed { reason, .. } => {
                self.running.send_if_current(&self.caller, id, true, || {
                    self.bus.emit_signal(
                        caller,
                        path,
                        INTERFACE,
                        FAILED,
                        &(id, reason_name(reason)),
                    )
                })
            }
            _ => Some(Ok(())),
        };
        match sent {
            Some(Ok(())) => Ok(()),
            // Not the running search any more, or its caller is gone.
            _ => Err(ClientGone),
        }
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

/// What a path to open is.
enum Target {
    Folder,
    /// A file, opened `O_PATH`.
    File(OwnedFd),
}

/// Looks at `path` on threads of its own, giving up after
/// [`FILE_SYSTEM_TIMEOUT`]: on a mount that doesn't answer, the lookup
/// blocks, and zbus answers every call on one executor.
async fn look_at(path: PathBuf) -> fdo::Result<Target> {
    let (looked, look) = mpsc::channel();
    let (answer_sender, answer) = oneshot::channel();
    spawn(move || {
        looked.send(target(&path)).ok();
    })?;
    // The lookup may never end; this one does.
    spawn(move || {
        answer_sender
            .send(look.recv_timeout(FILE_SYSTEM_TIMEOUT))
            .ok();
    })?;
    match answer.await {
        Ok(Ok(target)) => target,
        Ok(Err(RecvTimeoutError::Timeout)) => Err(fdo::Error::Timeout(
            "the file system did not answer".to_owned(),
        )),
        Ok(Err(RecvTimeoutError::Disconnected)) | Err(_) => {
            Err(fdo::Error::Failed("the lookup stopped".to_owned()))
        }
    }
}

fn spawn(work: impl FnOnce() + Send + 'static) -> fdo::Result<()> {
    thread::Builder::new()
        .name(LOOK_THREAD.to_owned())
        .spawn(work)
        .map(drop)
        .map_err(failed)
}

fn target(path: &Path) -> fdo::Result<Target> {
    if path.is_dir() {
        return Ok(Target::Folder);
    }
    rustix::fs::open(path, OFlags::PATH.union(OFlags::CLOEXEC), Mode::empty())
        .map(Target::File)
        .map_err(|error| fdo::Error::InvalidArgs(format!("{}: {error}", path.display())))
}

/// Asks the portal to open `file` with the user's default app.
async fn open_file(
    bus: &zbus::Connection,
    file: OwnedFd,
    token: Option<String>,
) -> fdo::Result<()> {
    let mut options: HashMap<&str, Value<'_>> = HashMap::new();
    if let Some(token) = token {
        options.insert(PORTAL_ACTIVATION_TOKEN, Value::from(token));
    }
    let portal = zbus::Proxy::new(bus, PORTAL_NAME, PORTAL_PATH, OPEN_URI_INTERFACE).await?;
    // No parent window: the launcher has closed by now.
    let request: zbus::zvariant::OwnedObjectPath = portal
        .call("OpenFile", &("", Fd::from(&file), options))
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

    #[test]
    fn only_the_running_search_sends_and_its_last_stage_ends_it() {
        let dropped = Cell::new(0);
        let running = Running::default();
        running.put(CALLER, 2, Dropped(&dropped), |_| ());
        assert_eq!(running.send_if_current(CALLER, 1, false, || "sent"), None);
        assert_eq!(running.send_if_current(OTHER, 2, false, || "sent"), None);
        assert_eq!(
            running.send_if_current(CALLER, 2, false, || "sent"),
            Some("sent")
        );
        assert!(running.is_current(CALLER, 2));
        assert_eq!(
            running.send_if_current(CALLER, 2, true, || "last"),
            Some("last")
        );
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
