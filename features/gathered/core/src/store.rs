use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Once, Weak};
use std::thread;

use nova_files_disk::change_writer::ChangeWriter;
use nova_files_disk::config_watch::{self, Changed, ConfigWatch};
use nova_files_disk::hand_edited::lock;
use nova_files_disk::home::Home;
use nova_files_disk::private_file::{self, Read};
use nova_files_gathered_api::{Event, Gathered, GatheredChange, GatheredFolder, Gathering, Holds};
use nova_files_messages::{ClientId, EventSink};

use super::file::{self, Edit};
use super::{FILE, guess, usual_folders};

const MAX_BYTES: usize = 256 * 1024;
const LOAD_THREAD: &str = "nova-files-gathered";
const SAVE_THREAD: &str = "nova-files-gathered-save";
const WHAT: &str = "gathered folders";

pub struct Store {
    shared: Arc<Shared>,
    writer: ChangeWriter<GatheredChange>,
    _watches: Arc<Mutex<Watches>>,
}

struct Shared {
    home: Option<Home>,
    started: Once,
    disk: Mutex<()>,
    folders: Arc<Mutex<Folders>>,
    watches: Weak<Mutex<Watches>>,
}

#[derive(Default)]
struct Folders {
    loaded: bool,
    gathered: Arc<Gathered>,
    listeners: BTreeMap<ClientId, Arc<dyn EventSink>>,
}

#[derive(Default)]
struct Watches {
    file: Option<ConfigWatch>,
    folders: BTreeMap<(Gathering, PathBuf, Holds), ConfigWatch>,
}

impl Store {
    pub fn open(home: Option<Home>) -> Self {
        let watches = Arc::default();
        let shared = Arc::new(Shared {
            home,
            started: Once::new(),
            disk: Mutex::new(()),
            folders: Arc::default(),
            watches: Arc::downgrade(&watches),
        });
        let saving = Arc::clone(&shared);
        Self {
            shared,
            writer: ChangeWriter::new(SAVE_THREAD, WHAT, move |change| saving.save(change)),
            _watches: watches,
        }
    }

    pub fn listen(&self, client: ClientId, events: &Arc<dyn EventSink>) {
        {
            let mut folders = lock(&self.shared.folders);
            let sent = if folders.loaded {
                events.deliver(Event::Gathered(Arc::clone(&folders.gathered)))
            } else {
                Ok(())
            };
            if sent.is_ok() {
                folders.listeners.insert(client, Arc::clone(events));
            }
        }
        if !self.shared.started.is_completed() {
            self.load();
        }
    }

    pub fn folders_of(
        &self,
        place: Gathering,
    ) -> impl FnOnce() -> Arc<[GatheredFolder]> + Send + 'static {
        let shared = Arc::clone(&self.shared);
        move || {
            shared.start();
            let folders = lock(&shared.folders);
            match place {
                Gathering::Projects => Arc::clone(&folders.gathered.projects),
                Gathering::Games => Arc::clone(&folders.gathered.games),
            }
        }
    }

    pub fn forget(&self, client: ClientId) {
        lock(&self.shared.folders).listeners.remove(&client);
    }

    pub fn change(&self, change: GatheredChange) {
        if self.shared.home.is_some() {
            self.writer.send(change);
        }
    }

    fn load(&self) {
        let shared = Arc::clone(&self.shared);
        let load = move || shared.start();
        let started = thread::Builder::new()
            .name(LOAD_THREAD.to_owned())
            .spawn(load.clone());
        if let Err(error) = started {
            log::warn!(
                "can't read the gathered folders on their own thread, reading here: {error}"
            );
            load();
        }
    }
}

impl Shared {
    fn start(self: &Arc<Self>) {
        self.started.call_once(|| {
            if let Some(home) = &self.home
                && let Some(watches) = self.watches.upgrade()
            {
                lock(&watches).file = start_watch(home, &Arc::downgrade(self));
            }
            self.first_read();
        });
    }

    fn first_read(&self) {
        let changed = {
            let _disk = lock(&self.disk);
            if let Some(home) = &self.home {
                write_usual(home);
            }
            self.read_holding_disk()
        };
        self.rewatch_and_tell(changed);
    }

    fn reload(&self) {
        let changed = {
            let _disk = lock(&self.disk);
            self.read_holding_disk()
        };
        self.rewatch_and_tell(changed);
    }

    fn read_holding_disk(&self) -> bool {
        let read = self
            .home
            .as_ref()
            .map_or_else(|| Some(Gathered::default()), read_folders);
        let mut folders = lock(&self.folders);
        let first = !folders.loaded;
        let gathered = match read {
            Some(gathered) => gathered,
            None if first => Gathered::default(),
            None => return false,
        };
        let changed = first || *folders.gathered != gathered;
        if changed {
            folders.gathered = Arc::new(gathered);
        }
        drop(folders);
        changed
    }

    fn rewatch_and_tell(&self, changed: bool) {
        self.rewatch();
        if !changed {
            return;
        }
        let mut folders = lock(&self.folders);
        folders.loaded = true;
        let event = Event::Gathered(Arc::clone(&folders.gathered));
        folders
            .listeners
            .retain(|_, events| events.deliver(event.clone()).is_ok());
    }

    fn rewatch(&self) {
        let Some(watches) = self.watches.upgrade() else {
            return;
        };
        let mut watches = lock(&watches);
        let gathered = Arc::clone(&lock(&self.folders).gathered);
        let wanted: BTreeSet<(Gathering, PathBuf, Holds)> = Gathering::ALL
            .into_iter()
            .flat_map(|place| {
                gathered
                    .of(place)
                    .iter()
                    .map(move |folder| (place, folder.path.clone(), folder.holds))
            })
            .collect();
        watches.folders.retain(|key, _| wanted.contains(key));
        for key in wanted {
            if watches.folders.contains_key(&key) {
                continue;
            }
            if let Some(watch) = watch_folder(&key, Arc::downgrade(&self.folders)) {
                watches.folders.insert(key, watch);
            }
        }
    }

    fn save(&self, change: GatheredChange) {
        let Some(home) = &self.home else {
            return;
        };
        let Some(change) = self.guessed(change) else {
            return;
        };
        let disk = lock(&self.disk);
        write_usual(home);
        let file = file_of(home);
        match private_file::saved_at(&file) {
            Ok(target) => make_change(&target, &home.folder, change),
            Err(error) => log::warn!("can't save {}: {error}", file.display()),
        }
        let changed = self.read_holding_disk();
        drop(disk);
        self.rewatch_and_tell(changed);
    }

    fn guessed(&self, change: GatheredChange) -> Option<Change> {
        Some(match change {
            GatheredChange::Add {
                place,
                path,
                holds,
                at,
                name,
            } => {
                if fs::metadata(&path).is_ok_and(|found| !found.is_dir()) {
                    log::warn!("{} not gathered: it is a file", path.display());
                    return None;
                }
                let holds = holds.unwrap_or_else(|| {
                    let known = lock(&self.folders).gathered.has(place, &path);
                    if known {
                        Holds::default()
                    } else {
                        guess(place, &path)
                    }
                });
                Change::Add {
                    place,
                    folder: GatheredFolder { path, holds, name },
                    at: at.and_then(|at| usize::try_from(at).ok()),
                }
            }
            GatheredChange::Remove { place, path } => Change::Remove { place, path },
        })
    }
}

enum Change {
    Add {
        place: Gathering,
        folder: GatheredFolder,
        at: Option<usize>,
    },
    Remove {
        place: Gathering,
        path: PathBuf,
    },
}

fn make_change(target: &Path, home: &Path, change: Change) {
    let Some(text) = read_text(target) else {
        return;
    };
    let edit = match change {
        Change::Add { place, folder, at } => file::add(&text, home, place, &folder, at),
        Change::Remove { place, path } => file::remove(&text, home, place, &path),
    };
    match edit {
        Edit::Changed(text) => {
            let mode = private_file::mode_to_keep(target);
            if let Err(error) = private_file::replace(target, text.as_bytes(), mode) {
                log::warn!("can't save {}: {error}", target.display());
            }
        }
        Edit::Unchanged => {}
        Edit::Refused(why) => log::warn!("{} not changed: {why}", target.display()),
    }
}
fn file_of(home: &Home) -> PathBuf {
    config_watch::folder(&home.config).join(FILE)
}

fn read_text(path: &Path) -> Option<String> {
    match private_file::read(path, MAX_BYTES) {
        Read::Bytes(bytes) => String::from_utf8(bytes)
            .map_err(|error| log::warn!("{} isn't UTF-8 text: {error}", path.display()))
            .ok(),
        Read::Missing => Some(String::new()),
        Read::TooBig => {
            log::warn!("{} is over {MAX_BYTES} bytes; not read", path.display());
            None
        }
        Read::Failed(error) => {
            log::warn!("can't read {}: {error}", path.display());
            None
        }
    }
}

fn read_folders(home: &Home) -> Option<Gathered> {
    let path = file_of(home);
    let text = read_text(&path)?;
    match file::read(&text, &home.folder) {
        Ok((gathered, problems)) => {
            for problem in problems {
                log::warn!("{}: {problem}", path.display());
            }
            Some(gathered)
        }
        Err(problem) => {
            log::warn!(
                "{}: {problem}; keeping the folders read before",
                path.display()
            );
            None
        }
    }
}

fn write_usual(home: &Home) {
    let path = file_of(home);
    if fs::symlink_metadata(&path).is_ok() {
        return;
    }
    let text = file::new_file(&home.folder, &usual_folders(home));
    if let Err(error) = private_file::replace(&path, text.as_bytes(), private_file::FILE_MODE) {
        log::warn!("can't write {}: {error}", path.display());
    }
}

fn watch_folder(
    (place, path, holds): &(Gathering, PathBuf, Holds),
    folders: Weak<Mutex<Folders>>,
) -> Option<ConfigWatch> {
    let place = *place;
    let tell = move || {
        if let Some(folders) = folders.upgrade() {
            let event = Event::GatheredChanged(place);
            lock(&folders)
                .listeners
                .retain(|_, events| events.deliver(event.clone()).is_ok());
        }
    };
    let started = match holds {
        Holds::Many => {
            let folder = path.clone();
            ConfigWatch::start(path.clone(), &[], move |changed| {
                if changes_a_folder(&folder, changed) {
                    tell();
                }
            })
        }
        Holds::One => {
            let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
                return None;
            };
            let name = name.to_owned();
            ConfigWatch::start(parent.to_owned(), &[], move |changed| {
                if changed.touches_os(&name) {
                    tell();
                }
            })
        }
    };
    started
        .map_err(|error| log::warn!("{} changing won't be seen: {error}", path.display()))
        .ok()
}

fn changes_a_folder(folder: &Path, changed: &Changed) -> bool {
    match changed {
        Changed::All => true,
        Changed::Files(names) => names
            .iter()
            .any(|name| fs::metadata(folder.join(name)).map_or(true, |found| found.is_dir())),
    }
}

fn start_watch(home: &Home, shared: &Weak<Shared>) -> Option<ConfigWatch> {
    let shared = Weak::clone(shared);
    let started = ConfigWatch::start(
        config_watch::folder(&home.config),
        &[FILE],
        move |changed| {
            if changed.touches(FILE)
                && let Some(shared) = shared.upgrade()
            {
                shared.reload();
            }
        },
    );
    started
        .map_err(|error| log::warn!("gathered folders changed on disk won't be seen: {error}"))
        .ok()
}

#[cfg(test)]
mod tests;
