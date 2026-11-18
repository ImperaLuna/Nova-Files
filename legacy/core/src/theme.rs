mod file;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

use nova_files_api::{
    Appearance, ClientGone, ClientId, DesktopLook, Envelope, Event, EventSink, Mode, Palette, Theme,
};

use crate::config_watch::{self, ConfigWatch};
use crate::hand_edited::{self, Reread, lock};
use crate::settings::Store as SettingsStore;
use file::ThemeFile;

const FILE: &str = "theme.toml";
const MAX_BYTES: usize = 64 * 1024;
const SETTINGS_LISTENER: ClientId = ClientId(u64::MAX);

pub struct Store {
    shared: Arc<Mutex<Shared>>,
    _watch: Option<ConfigWatch>,
}

struct Shared {
    theme: Arc<Theme>,
    file: ThemeFile,
    appearance: Appearance,
    desktop: DesktopLook,
    listeners: BTreeMap<ClientId, Arc<dyn EventSink>>,
    path: Option<PathBuf>,
    version: u64,
}

impl Store {
    pub fn open(config: Option<&Path>) -> Self {
        let folder = config.map(config_watch::folder);
        let path = folder.as_ref().map(|folder| folder.join(FILE));
        let shared = Arc::new(Mutex::new(Shared {
            theme: Arc::new(resolve(
                &ThemeFile::default(),
                Appearance::default(),
                DesktopLook::default(),
            )),
            file: ThemeFile::default(),
            appearance: Appearance::default(),
            desktop: DesktopLook::default(),
            listeners: BTreeMap::new(),
            path,
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
                .map_err(|error| log::warn!("theme changes on disk won't be seen: {error}"))
                .ok()
        });
        reload(&Arc::downgrade(&shared));
        Self {
            shared,
            _watch: watch,
        }
    }

    pub fn follow(&self, settings: &SettingsStore) {
        let follower: Arc<dyn EventSink> = Arc::new(Follower(Arc::downgrade(&self.shared)));
        settings.listen(SETTINGS_LISTENER, &follower);
    }

    pub fn follow_desktop(&self, look: DesktopLook) {
        let mut shared = lock(&self.shared);
        shared.desktop = look;
        shared.update();
    }

    pub fn listen(&self, client: ClientId, events: &Arc<dyn EventSink>) {
        let mut shared = lock(&self.shared);
        if events
            .deliver(Event::Theme(Arc::clone(&shared.theme)))
            .is_ok()
        {
            shared.listeners.insert(client, Arc::clone(events));
        }
    }

    pub fn forget(&self, client: ClientId) {
        lock(&self.shared).listeners.remove(&client);
    }
}

impl Shared {
    fn update(&mut self) {
        let theme = resolve(&self.file, self.appearance, self.desktop);
        if theme == *self.theme {
            return;
        }
        self.theme = Arc::new(theme);
        let theme = &self.theme;
        self.listeners
            .retain(|_, events| events.deliver(Event::Theme(Arc::clone(theme))).is_ok());
    }
}

struct Follower(Weak<Mutex<Shared>>);

impl EventSink for Follower {
    fn deliver_envelope(&self, envelope: Envelope) -> Result<(), ClientGone> {
        let Some(event) = envelope.taken_as::<Event>() else {
            return Ok(());
        };
        let shared = self.0.upgrade().ok_or(ClientGone)?;
        if let Event::Settings(settings) = event {
            let mut locked = lock(&shared);
            locked.appearance = settings.appearance;
            locked.update();
        }
        Ok(())
    }
}

fn resolve(file: &ThemeFile, appearance: Appearance, desktop: DesktopLook) -> Theme {
    let mode = match appearance {
        Appearance::Light => Mode::Light,
        Appearance::Dark => Mode::Dark,
        Appearance::System => desktop.mode.unwrap_or(Mode::Dark),
    };
    let mut palette = Palette::carbonfox(mode);
    if let Some(accent) = desktop.accent {
        palette.primary = accent;
    }
    Theme {
        mode,
        palette: file.for_mode(mode).over(palette),
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

fn reload(weak: &Weak<Mutex<Shared>>) {
    hand_edited::reload(weak, read);
}

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
