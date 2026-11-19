use std::collections::HashMap;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use futures::channel::oneshot;

use nova_files_api::{ClientGone, Connector, Envelope, EventSink, WindowRequest};
use nova_files_open_api as open_api;
use nova_files_search_api::{
    Command as SearchCommand, Event as SearchEvent, Hit, Matched, SearchFailed, SearchId,
};
use rustix::fs::{Mode, OFlags};
use zbus::fdo;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{Fd, OwnedValue, Value};

use crate::application::keep_core_up;
use crate::files::ask_core;
use crate::holds::{Hold, Holds};

pub const INTERFACE: &str = "io.github.ImperaLuna.NovaFiles.Search1";

pub const PORTAL_NAME: &str = "org.freedesktop.portal.Desktop";
pub const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
pub const OPEN_URI_INTERFACE: &str = "org.freedesktop.portal.OpenURI";

pub const ACTIVATION_TOKEN: &str = "activation-token";
const PORTAL_ACTIVATION_TOKEN: &str = "activation_token";

const FILE_SYSTEM_TIMEOUT: Duration = Duration::from_secs(2);
const LOOK_THREAD: &str = "nova-files-dbus-look";

const FOUND: &str = "Found";
const FAILED: &str = "Failed";

type Search = Hold;

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

    fn end(&self, caller: &str, id: u64) {
        self.send_if_current(caller, id, true, || ());
    }

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

impl<F: Fn(WindowRequest)> Searches<F> {
    fn open_window(&self, request: WindowRequest) -> fdo::Result<()> {
        keep_core_up(&self.connector)?;
        (self.on_open)(request);
        Ok(())
    }
}

#[zbus::interface(name = "io.github.ImperaLuna.NovaFiles.Search1")]
impl<F: Fn(WindowRequest) + Send + Sync + 'static> Searches<F> {
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
        let command = SearchCommand::Search {
            id: SearchId(id),
            query,
            limit,
        };
        self.running
            .put(caller.as_str(), id, search, |search| search.send(command))
            .map_err(failed)
    }

    fn cancel(&self, #[zbus(header)] header: Header<'_>, id: u64) {
        let caller = header.sender().map(ToString::to_string);
        drop(header);
        if let Some(caller) = caller {
            self.running.end(&caller, id);
        }
    }

    async fn open(
        &self,
        #[zbus(connection)] bus: &zbus::Connection,
        path: &str,
        platform_data: HashMap<String, OwnedValue>,
    ) -> fdo::Result<()> {
        let path = absolute(path)?;
        match look_at(path.clone()).await? {
            Target::Folder => self.open_window(WindowRequest::on(path)),
            Target::File(file) => {
                let token = token(&platform_data);
                let command = open_api::Command::OpenFile {
                    path,
                    activation: token.clone(),
                };
                match ask_core(&self.connector, command).await? {
                    open_api::Event::Opened { .. } => Ok(()),
                    open_api::Event::NotOpened { .. } => open_file(bus, file, token).await,
                    open_api::Event::OpenWith(_) => {
                        Err(failed("the core gave an unexpected answer"))
                    }
                }
            }
        }
    }

    fn show_in_folder(
        &self,
        path: &str,
        platform_data: HashMap<String, OwnedValue>,
    ) -> fdo::Result<()> {
        drop(platform_data);
        let request = WindowRequest::showing(&absolute(path)?)
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("no folder holds {path}")))?;
        self.open_window(request)
    }

    #[zbus(signal)]
    async fn found(
        emitter: &SignalEmitter<'_>,
        id: u64,
        hits: Vec<(String, String)>,
        done: bool,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn failed(emitter: &SignalEmitter<'_>, id: u64, reason: String) -> zbus::Result<()>;
}

struct Stages {
    bus: zbus::blocking::Connection,
    path: String,
    caller: String,
    id: u64,
    running: Arc<Running<Search>>,
}

impl EventSink for Stages {
    fn deliver_envelope(&self, envelope: Envelope) -> Result<(), ClientGone> {
        let Some(event) = envelope.taken_as::<SearchEvent>() else {
            return Ok(());
        };
        let (caller, path, id) = (Some(self.caller.as_str()), self.path.as_str(), self.id);
        let sent = match event {
            SearchEvent::Found { hits, done, .. } => {
                let hits: Vec<(String, String)> = hits.iter().filter_map(wire).collect();
                self.running.send_if_current(&self.caller, id, done, || {
                    self.bus
                        .emit_signal(caller, path, INTERFACE, FOUND, &(id, hits, done))
                })
            }
            SearchEvent::SearchFailed { reason, .. } => {
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
            SearchEvent::FindFailed { .. } => Some(Ok(())),
        };
        match sent {
            Some(Ok(())) => Ok(()),
            _ => Err(ClientGone),
        }
    }
}

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

fn token(platform_data: &HashMap<String, OwnedValue>) -> Option<String> {
    let value = platform_data.get(ACTIVATION_TOKEN)?;
    String::try_from(value.try_clone().ok()?).ok()
}

enum Target {
    Folder,
    File(OwnedFd),
}

async fn look_at(path: PathBuf) -> fdo::Result<Target> {
    let (looked, look) = mpsc::channel();
    let (answer_sender, answer) = oneshot::channel();
    spawn(move || {
        looked.send(target(&path)).ok();
    })?;
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

    struct Dropped<'a>(&'a Cell<u32>);

    impl Drop for Dropped<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn interface_name_matches_the_constant() {
        assert_eq!(
            Searches::<fn(nova_files_api::WindowRequest)>::name().as_str(),
            INTERFACE
        );
    }
}
