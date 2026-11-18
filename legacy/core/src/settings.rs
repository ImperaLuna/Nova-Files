mod file;

use std::collections::BTreeMap;
use std::mem;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, Weak};
use std::thread;

use nova_files_api::{ClientId, EventSink};
use nova_files_settings_api::{Event, SettingChange, Settings};

use crate::config_watch::{self, ConfigWatch};
use crate::hand_edited::{self, Reread, lock};
use crate::private_file::{self, Read};

const FILE: &str = "settings.toml";
const MAX_BYTES: usize = 64 * 1024;
const SAVE_THREAD: &str = "nova-files-settings";

pub struct Store {
    shared: Arc<Mutex<Shared>>,
    _watch: Option<ConfigWatch>,
}

struct Shared {
    settings: Arc<Settings>,
    listeners: BTreeMap<ClientId, Arc<dyn EventSink>>,
    path: Option<PathBuf>,
    writer: Option<Sender<SettingChange>>,
    unsaved: usize,
    read_when_saved: bool,
    version: u64,
}

impl Store {
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
        let watch = folder.and_then(|folder| {
            let weak = Arc::downgrade(&shared);
            let started = ConfigWatch::start(folder, &[FILE], move |changed| {
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

    pub fn current(&self) -> Arc<Settings> {
        Arc::clone(&lock(&self.shared).settings)
    }

    pub fn listen(&self, client: ClientId, events: &Arc<dyn EventSink>) {
        let mut shared = lock(&self.shared);
        if events
            .deliver(Event::Settings(Arc::clone(&shared.settings)))
            .is_ok()
        {
            shared.listeners.insert(client, Arc::clone(events));
        }
    }

    pub fn forget(&self, client: ClientId) {
        lock(&self.shared).listeners.remove(&client);
    }

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
                self.unsaved = 0;
                self.read_when_saved = false;
            }
            None => {}
        }
    }

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
}

impl Reread for Shared {
    type Read = Settings;

    fn path(&self) -> Option<PathBuf> {
        self.path.clone()
    }

    fn version(&self) -> u64 {
        self.version
    }

    fn take(&mut self, settings: Settings, version: u64) -> bool {
        if self.unsaved > 0 {
            self.read_when_saved = true;
            return false;
        }
        if self.version != version {
            return true;
        }
        if settings == *self.settings {
            self.version += 1;
        } else {
            self.set(settings);
        }
        false
    }
}

fn reload(weak: &Weak<Mutex<Shared>>) {
    hand_edited::reload(weak, read);
}

fn read(path: &Path) -> Settings {
    let Some(text) = hand_edited::read_text(path, MAX_BYTES) else {
        return Settings::default();
    };
    let (settings, problems) = file::read(&text);
    for problem in problems {
        log::warn!("{}: {problem}", path.display());
    }
    settings
}

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

fn save_each(path: &Path, changes: &Receiver<SettingChange>, shared: &Weak<Mutex<Shared>>) {
    for change in changes {
        save(path, &change);
        let Some(strong) = shared.upgrade() else {
            return;
        };
        let mut locked = lock(&strong);
        locked.unsaved = locked.unsaved.saturating_sub(1);
        locked.version += 1;
        let read_now = locked.unsaved == 0 && mem::take(&mut locked.read_when_saved);
        drop(locked);
        if read_now {
            reload(shared);
        }
    }
}

fn save(path: &Path, change: &SettingChange) {
    let target = match private_file::saved_at(path) {
        Ok(target) => target,
        Err(error) => {
            log::warn!("can't save {}: {error}", path.display());
            return;
        }
    };
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
    let mode = private_file::mode_to_keep(&target);
    if let Err(error) = private_file::replace(&target, edited.as_bytes(), mode) {
        log::warn!("can't save {}: {error}", target.display());
    }
}

#[cfg(test)]
mod tests;
