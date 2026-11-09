//! The settings: kept in memory, read from and saved to
//! `$XDG_CONFIG_HOME/nova-files/settings.toml`, and sent to the clients
//! that asked for them each time they change.
//!
//! Three threads change them, all under one lock, so a new value, the
//! clients being told and the save being queued happen together and in
//! order:
//! - the core's loop, for a client's [`SettingChange`]: the value changes,
//!   every listening client is told, and the change is queued for the
//!   writer;
//! - the config watcher (`config_watch`), when the file changes on disk:
//!   it is read again (see `file`) and clients are told only when the
//!   settings differ, so the core's own save coming back sends nothing;
//! - the writer, which makes each queued change in the file's current text
//!   and saves it whole (see `private_file`) with the file's own mode, so
//!   edits made by hand in the meantime stay. A file that is a symlink is
//!   saved where it points. A change that can't be saved (a read-only
//!   file, or one that isn't TOML while the user fixes it) is logged and
//!   kept in memory until the file next changes on disk: the file wins.
//!
//! While saves are queued, the file on disk is older than the settings in
//! memory, so a change the watcher sees then is put off until the last
//! save is done, and read then. Reads happen outside the lock, so each
//! carries the settings' version from before it read: a read that finds
//! the settings changed since (a save done, a client's change, a newer
//! read) is stale and reads again rather than roll them back.

mod file;

use std::collections::BTreeMap;
use std::fs;
use std::mem;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::thread;

use nova_files_api::{ClientId, Event, EventSink, SettingChange, Settings};

use crate::config_watch::{self, ConfigWatch};
use crate::private_file::{self, Read};

/// The file, in the config folder.
const FILE: &str = "settings.toml";
/// A settings file this big is not one a person wrote; it is never read.
const MAX_BYTES: usize = 64 * 1024;
const SAVE_THREAD: &str = "nova-files-settings";
/// The permission bits of a file's mode, kept across a save.
const PERMISSION_BITS: u32 = 0o7777;

/// The settings and who hears about them.
pub struct Store {
    shared: Arc<Mutex<Shared>>,
    /// Stops with the store.
    _watch: Option<ConfigWatch>,
}

struct Shared {
    settings: Arc<Settings>,
    /// The clients that asked for the settings.
    listeners: BTreeMap<ClientId, Arc<dyn EventSink>>,
    /// Where they are saved; `None` without a config folder, which keeps
    /// them in memory only.
    path: Option<PathBuf>,
    /// The writer's queue, started on the first change.
    writer: Option<Sender<SettingChange>>,
    /// Changes queued and not yet saved.
    unsaved: usize,
    /// The file changed while saves were queued; read it once they're done.
    read_when_saved: bool,
    /// Counts every change to the settings and every save, so a read can
    /// tell whether it is still the newest word.
    version: u64,
}

impl Store {
    /// The settings saved in the config home `config`, followed from then
    /// on. Without `config` they stay at their defaults, in memory.
    pub fn open(config: Option<&Path>) -> Self {
        let folder = config.map(config_watch::folder);
        let path = folder.as_ref().map(|folder| folder.join(FILE));
        let shared = Arc::new(Mutex::new(Shared {
            settings: Arc::default(),
            listeners: BTreeMap::new(),
            path,
            writer: None,
            unsaved: 0,
            read_when_saved: false,
            version: 0,
        }));
        // The watch goes in before the first read, so no change is missed.
        let watch = folder.and_then(|folder| {
            let weak = Arc::downgrade(&shared);
            let started = ConfigWatch::start(folder, move |changed| {
                if changed.touches(FILE) {
                    reload(&weak);
                }
            });
            started
                .map_err(|error| log::warn!("settings changed on disk won't be seen: {error}"))
                .ok()
        });
        reload(&Arc::downgrade(&shared));
        Self {
            shared,
            _watch: watch,
        }
    }

    /// Sends the settings to `client` now and each time they change.
    pub fn listen(&self, client: ClientId, events: &Arc<dyn EventSink>) {
        let mut shared = lock(&self.shared);
        if events
            .deliver(Event::Settings(Arc::clone(&shared.settings)))
            .is_ok()
        {
            shared.listeners.insert(client, Arc::clone(events));
        }
    }

    /// Stops sending the settings to `client`.
    pub fn forget(&self, client: ClientId) {
        lock(&self.shared).listeners.remove(&client);
    }

    /// Makes `change`, tells the listening clients and saves it.
    pub fn change(&self, change: SettingChange) {
        let mut shared = lock(&self.shared);
        let changed = Settings::clone(&shared.settings).with(change.clone());
        if changed == *shared.settings {
            return;
        }
        shared.set(changed);
        shared.queue(change, &Arc::downgrade(&self.shared));
    }
}

impl Shared {
    /// Queues `change` for the writer, starting it on the first change.
    fn queue(&mut self, change: SettingChange, this: &Weak<Mutex<Self>>) {
        let Some(path) = self.path.clone() else {
            return;
        };
        if self.writer.is_none() {
            self.writer = start_writer(path, Weak::clone(this));
        }
        let sent = self.writer.as_ref().map(|queue| queue.send(change));
        match sent {
            Some(Ok(())) => self.unsaved += 1,
            Some(Err(_)) => {
                log::warn!("the settings writer stopped; settings aren't saved");
                self.writer = None;
                // The changes it held are lost; the file is the newest
                // word again.
                self.unsaved = 0;
                self.read_when_saved = false;
            }
            None => {}
        }
    }

    /// Keeps `settings` and tells every listening client; a client that
    /// stopped listening is dropped.
    fn set(&mut self, settings: Settings) {
        self.version += 1;
        self.settings = Arc::new(settings);
        let settings = &self.settings;
        self.listeners.retain(|_, events| {
            events
                .deliver(Event::Settings(Arc::clone(settings)))
                .is_ok()
        });
    }

    /// Takes `settings`, read from the file at `version`; says whether the
    /// read was stale (the settings changed while it ran) and must be done
    /// again. Put off while saves are queued.
    fn take(&mut self, settings: Settings, version: u64) -> bool {
        if self.unsaved > 0 {
            self.read_when_saved = true;
            return false;
        }
        if self.version != version {
            return true;
        }
        // Every read taken is newer than any read that started before it,
        // even one that changed nothing.
        if settings == *self.settings {
            self.version += 1;
        } else {
            self.set(settings);
        }
        false
    }
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    // A panic while holding the lock left whole values behind: each field
    // is replaced in one step.
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Reads the file again and takes what it says, unless saves are queued;
/// then it is read once they're done.
fn reload(weak: &Weak<Mutex<Shared>>) {
    let Some(shared) = weak.upgrade() else {
        return;
    };
    loop {
        let (path, version) = {
            let locked = lock(&shared);
            let Some(path) = locked.path.clone() else {
                return;
            };
            (path, locked.version)
        };
        // Read outside the lock: a slow disk never holds up the core.
        let settings = read(&path);
        if !lock(&shared).take(settings, version) {
            return;
        }
    }
}

/// The settings in the file at `path`; defaults for what it lacks.
fn read(path: &Path) -> Settings {
    let text = match private_file::read(path, MAX_BYTES) {
        Read::Bytes(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Read::Missing => return Settings::default(),
        Read::TooBig => {
            log::warn!(
                "{} is over {MAX_BYTES} bytes, using defaults",
                path.display()
            );
            return Settings::default();
        }
        Read::Failed(error) => {
            log::warn!("can't read {}, using defaults: {error}", path.display());
            return Settings::default();
        }
    };
    let (settings, problems) = file::read(&text);
    for problem in problems {
        log::warn!("{}: {problem}", path.display());
    }
    settings
}

/// Starts the thread that saves each change it is sent to `path`, in
/// order, or none when no thread can start.
fn start_writer(path: PathBuf, shared: Weak<Mutex<Shared>>) -> Option<Sender<SettingChange>> {
    let (queue, changes) = mpsc::channel();
    let started = thread::Builder::new()
        .name(SAVE_THREAD.to_owned())
        .spawn(move || save_each(&path, &changes, &shared));
    match started {
        Ok(_) => Some(queue),
        Err(error) => {
            log::warn!("can't start the settings writer, settings aren't saved: {error}");
            None
        }
    }
}

/// Saves each change from `changes` to `path`, in order, until the store
/// is gone.
fn save_each(path: &Path, changes: &Receiver<SettingChange>, shared: &Weak<Mutex<Shared>>) {
    for change in changes {
        save(path, &change);
        let Some(strong) = shared.upgrade() else {
            return;
        };
        let mut locked = lock(&strong);
        locked.unsaved = locked.unsaved.saturating_sub(1);
        locked.version += 1;
        // Once the last save is done, a read put off for it is due.
        let read_now = locked.unsaved == 0 && mem::take(&mut locked.read_when_saved);
        drop(locked);
        if read_now {
            reload(shared);
        }
    }
}

/// Makes `change` in the file at `path`, or logs why it can't.
fn save(path: &Path, change: &SettingChange) {
    let target = where_saved(path);
    let text = match private_file::read(&target, MAX_BYTES) {
        Read::Bytes(bytes) => {
            let Ok(text) = String::from_utf8(bytes) else {
                log::warn!(
                    "{} isn't UTF-8 text; the change isn't saved",
                    target.display()
                );
                return;
            };
            text
        }
        Read::Missing => String::new(),
        Read::TooBig => {
            log::warn!(
                "{} is over {MAX_BYTES} bytes; the change isn't saved",
                target.display()
            );
            return;
        }
        Read::Failed(error) => {
            log::warn!(
                "can't read {}; the change isn't saved: {error}",
                target.display()
            );
            return;
        }
    };
    let Some(edited) = file::edit(&text, change) else {
        log::warn!("{} isn't TOML; the change isn't saved", target.display());
        return;
    };
    // A file the user made keeps its mode; a new one is private.
    let mode = fs::metadata(&target).map_or(private_file::FILE_MODE, |metadata| {
        metadata.permissions().mode() & PERMISSION_BITS
    });
    if let Err(error) = private_file::replace(&target, edited.as_bytes(), mode) {
        log::warn!("can't save {}: {error}", target.display());
    }
}

/// Where a save of `path` goes: the file a symlink points to, so a linked
/// settings file stays linked; `path` itself otherwise.
fn where_saved(path: &Path) -> PathBuf {
    let is_link = fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_symlink());
    if is_link {
        // A link to nowhere is saved over: there is nothing to keep.
        fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
    } else {
        path.to_owned()
    }
}

#[cfg(test)]
mod tests;
