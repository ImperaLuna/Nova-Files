//! The theme: the colors every window draws with, worked out from
//! `$XDG_CONFIG_HOME/nova-files/theme.toml` (see `file`), the appearance
//! setting and Carbonfox (see `defaults`), and sent to the clients that
//! asked for it each time it changes.
//!
//! The store follows the theme file through its own config watch, and the
//! settings by listening to them like a client does. Both come in on other
//! threads; each takes the store's lock, works the theme out again and
//! tells the clients only when it differs, so a save that changes another
//! setting sends nothing. The file is read as the settings' is (see
//! `hand_edited`).
//!
//! The settings store's lock is taken before this one, never after: the
//! settings tell the theme while holding theirs, and the theme never calls
//! into the settings.

mod defaults;
mod file;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

use nova_files_api::{Appearance, ClientGone, ClientId, Event, EventSink, Mode, Theme};

use crate::config_watch::{self, ConfigWatch};
use crate::hand_edited::{self, Reread, lock};
use crate::settings::Store as SettingsStore;
use file::ThemeFile;

/// The file, in the config folder.
const FILE: &str = "theme.toml";
/// A theme file this big is not one a person or a theming tool wrote.
const MAX_BYTES: usize = 64 * 1024;
/// The store's own place among the settings' listeners. Clients are
/// counted up from 0, so no client ever has it.
const SETTINGS_LISTENER: ClientId = ClientId(u64::MAX);

/// The theme and who hears about it.
pub struct Store {
    shared: Arc<Mutex<Shared>>,
    /// Stops with the store.
    _watch: Option<ConfigWatch>,
}

struct Shared {
    theme: Arc<Theme>,
    /// What the theme file sets.
    file: ThemeFile,
    appearance: Appearance,
    /// The clients that asked for the theme.
    listeners: BTreeMap<ClientId, Arc<dyn EventSink>>,
    /// The theme file; `None` without a config folder.
    path: Option<PathBuf>,
    /// Counts the reads taken, so a read can tell whether it is still the
    /// newest word.
    version: u64,
}

impl Store {
    /// The theme from the config home `config`, followed from then on.
    /// Without `config` it is Carbonfox.
    pub fn open(config: Option<&Path>) -> Self {
        let folder = config.map(config_watch::folder);
        let path = folder.as_ref().map(|folder| folder.join(FILE));
        let shared = Arc::new(Mutex::new(Shared {
            theme: Arc::new(resolve(&ThemeFile::default(), Appearance::default())),
            file: ThemeFile::default(),
            appearance: Appearance::default(),
            listeners: BTreeMap::new(),
            path,
            version: 0,
        }));
        // The watch goes in before the first read, so no change is missed.
        let watch = folder.and_then(|folder| {
            let weak = Arc::downgrade(&shared);
            let started = ConfigWatch::start(folder, &[FILE], move |changed| {
                if changed.touches(FILE) {
                    reload(&weak);
                }
            });
            started
                .map_err(|error| log::warn!("theme changes on disk won't be seen: {error}"))
                .ok()
        });
        reload(&Arc::downgrade(&shared));
        Self {
            shared,
            _watch: watch,
        }
    }

    /// Follows `settings` from now on, for the appearance.
    pub fn follow(&self, settings: &SettingsStore) {
        let follower: Arc<dyn EventSink> = Arc::new(Follower(Arc::downgrade(&self.shared)));
        settings.listen(SETTINGS_LISTENER, &follower);
    }

    /// Sends the theme to `client` now and each time it changes.
    pub fn listen(&self, client: ClientId, events: &Arc<dyn EventSink>) {
        let mut shared = lock(&self.shared);
        if events
            .deliver(Event::Theme(Arc::clone(&shared.theme)))
            .is_ok()
        {
            shared.listeners.insert(client, Arc::clone(events));
        }
    }

    /// Stops sending the theme to `client`.
    pub fn forget(&self, client: ClientId) {
        lock(&self.shared).listeners.remove(&client);
    }
}

impl Shared {
    /// Works the theme out again, and tells every listening client when it
    /// changed; a client that stopped listening is dropped.
    fn update(&mut self) {
        let theme = resolve(&self.file, self.appearance);
        if theme == *self.theme {
            return;
        }
        self.theme = Arc::new(theme);
        let theme = &self.theme;
        self.listeners
            .retain(|_, events| events.deliver(Event::Theme(Arc::clone(theme))).is_ok());
    }
}

/// Hears the settings for the store.
struct Follower(Weak<Mutex<Shared>>);

impl EventSink for Follower {
    fn deliver(&self, event: Event) -> Result<(), ClientGone> {
        let shared = self.0.upgrade().ok_or(ClientGone)?;
        if let Event::Settings(settings) = event {
            let mut locked = lock(&shared);
            locked.appearance = settings.appearance;
            locked.update();
        }
        Ok(())
    }
}

/// The theme for `appearance` with what `file` sets over Carbonfox.
fn resolve(file: &ThemeFile, appearance: Appearance) -> Theme {
    let mode = match appearance {
        Appearance::Light => Mode::Light,
        // The desktop's preference isn't known yet: Carbonfox is dark.
        Appearance::Dark | Appearance::System => Mode::Dark,
    };
    Theme {
        mode,
        palette: file.for_mode(mode).over(defaults::carbonfox(mode)),
    }
}

impl Reread for Shared {
    type Read = ThemeFile;

    fn path(&self) -> Option<PathBuf> {
        self.path.clone()
    }

    fn version(&self) -> u64 {
        self.version
    }

    /// Takes `file`, read at `version`; says whether the read was stale
    /// and must be done again.
    fn take(&mut self, file: ThemeFile, version: u64) -> bool {
        if self.version != version {
            return true;
        }
        self.version += 1;
        self.file = file;
        self.update();
        false
    }
}

/// Reads the file again and takes what it says.
fn reload(weak: &Weak<Mutex<Shared>>) {
    hand_edited::reload(weak, read);
}

/// What the theme file at `path` sets; nothing where it can't be read.
fn read(path: &Path) -> ThemeFile {
    let Some(text) = hand_edited::read_text(path, MAX_BYTES) else {
        return ThemeFile::default();
    };
    let (file, problems) = file::read(&text);
    for problem in problems {
        log::warn!("{}: {problem}", path.display());
    }
    file
}

#[cfg(test)]
mod tests;
