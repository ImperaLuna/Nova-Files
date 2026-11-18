//! The gathered folders as the core keeps them: read from the file,
//! followed on disk and sent to the clients that asked, each time they
//! change.
//!
//! Nothing is read until the first client asks, and then on a thread of
//! its own, so a core that only serves a file picker reads nothing. That
//! thread watches the config folder (see `config_watch`), writes the file
//! with the usual folders when there is none (see `gathered`), then reads
//! it and tells every client that asked so far.
//!
//! Every read of the disk and every save holds one lock, `disk`, from the
//! read until what it found is kept. Clients are told under the folders'
//! own lock, in the order they changed. A file that can't be read, or isn't TOML while the user
//! fixes it, keeps the folders last read: a passing error never empties a
//! place, and a change is never saved over it.
//!
//! Each gathered folder is watched too (see [`watch_folder`]); when what
//! one brings may have changed, the clients hear [`Event::GatheredChanged`]
//! for its place and list it again if they show it. The watches follow the
//! folders: each read that changes them adds and stops watches to match.
//!
//! A client's change goes to a writer thread (see `change_writer`), so the
//! core's loop never waits on the disk; it guesses what an added folder holds when the
//! client didn't say, edits the file's current text, saves it whole with
//! its mode (through a symlink, to where it points), then reads it again.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Once, Weak};
use std::thread;

use nova_files_api::{
    ClientId, Event, EventSink, Gathered, GatheredChange, GatheredFolder, Gathering, Holds,
};

use super::file::{self, Edit};
use super::{FILE, guess, usual_folders};
use crate::change_writer::ChangeWriter;
use crate::config_watch::{self, Changed, ConfigWatch};
use crate::hand_edited::lock;
use crate::places::Home;
use crate::private_file::{self, Read};

/// A gathered folders file this big is not one a person wrote; it is
/// neither read nor saved.
const MAX_BYTES: usize = 256 * 1024;
const LOAD_THREAD: &str = "nova-files-gathered";
const SAVE_THREAD: &str = "nova-files-gathered-save";
/// What the writer saves, for the log.
const WHAT: &str = "gathered folders";

/// The gathered folders and who hears about them.
pub struct Store {
    shared: Arc<Shared>,
    /// Saves each change, on a thread of its own.
    writer: ChangeWriter<GatheredChange>,
    /// The watches on the file and the gathered folders, which stop with
    /// the store.
    _watches: Arc<Mutex<Watches>>,
}

struct Shared {
    home: Option<Home>,
    /// Runs the watch on the file and the first read, once.
    started: Once,
    /// Held from each read of the disk until what it found is kept, and
    /// across each save.
    disk: Mutex<()>,
    /// Shared with the folders' watches, which tell its listeners.
    folders: Arc<Mutex<Folders>>,
    watches: Weak<Mutex<Watches>>,
}

#[derive(Default)]
struct Folders {
    /// The clients have been sent the folders once.
    loaded: bool,
    gathered: Arc<Gathered>,
    /// The clients that asked for the folders.
    listeners: BTreeMap<ClientId, Arc<dyn EventSink>>,
}

/// The watch on the file, and one on each gathered folder.
#[derive(Default)]
struct Watches {
    file: Option<ConfigWatch>,
    folders: BTreeMap<(Gathering, PathBuf, Holds), ConfigWatch>,
}

impl Store {
    /// The folders gathered by the user of `home`, read once a client
    /// asks. Without `home` both places stay empty.
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

    /// Sends the folders to `client` once they are read (now, when they
    /// already are), and each time they change.
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

    /// What gives the folders `place` gathers from, read first (and
    /// followed from then on) if no client asked yet; for a listing's
    /// thread, as that read may wait on the disk. Their folders are
    /// watched before it returns.
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

    /// Stops sending the folders to `client`.
    pub fn forget(&self, client: ClientId) {
        lock(&self.shared.folders).listeners.remove(&client);
    }

    /// Makes `change` in the file and tells the listening clients, on the
    /// writer's thread.
    pub fn change(&self, change: GatheredChange) {
        if self.shared.home.is_some() {
            self.writer.send(change);
        }
    }

    /// Starts following the file, on a thread of its own.
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
    /// The first time: starts the watch on the file, writes the usual
    /// folders when there is no file, reads it and watches its folders.
    /// A caller that comes while another runs it waits until it is done.
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

    /// Writes the usual folders when there is no file, then reads it.
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

    /// Reads the file again and tells the listening clients when the
    /// folders changed.
    fn reload(&self) {
        let changed = {
            let _disk = lock(&self.disk);
            self.read_holding_disk()
        };
        self.rewatch_and_tell(changed);
    }

    /// Reads the file into the folders; true when the clients must hear.
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

    /// Watches the folders, then sends them to the listening clients when
    /// `changed`, so a client hears of them only once they are watched.
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

    /// Watches each folder gathered now (see [`watch_folder`]), keeping
    /// the watches already in place, stopping those on folders gone and
    /// trying again any that couldn't start before. It runs outside the
    /// disk lock and takes the folders under the watches' lock.
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

    /// Makes `change` in the file, then reads it again. A file that can't
    /// be read is left as it is. The first change of a new user writes the
    /// usual folders first, as the first read would.
    fn save(&self, change: GatheredChange) {
        let Some(home) = &self.home else {
            return;
        };
        let change = self.guessed(change);
        let disk = lock(&self.disk);
        write_usual(home);
        let target = private_file::saved_at(&file_of(home));
        if let Some(text) = read_text(&target) {
            let edit = match change {
                Change::Add { place, folder, at } => {
                    file::add(&text, &home.folder, place, &folder, at)
                }
                Change::Remove { place, path } => file::remove(&text, &home.folder, place, &path),
            };
            match edit {
                Edit::Changed(text) => {
                    let mode = private_file::mode_to_keep(&target);
                    if let Err(error) = private_file::replace(&target, text.as_bytes(), mode) {
                        log::warn!("can't save {}: {error}", target.display());
                    }
                }
                Edit::Unchanged => {}
                Edit::Refused(why) => log::warn!("{} not changed: {why}", target.display()),
            }
        }
        let changed = self.read_holding_disk();
        drop(disk);
        self.rewatch_and_tell(changed);
    }

    /// `change` with what an added folder holds: what the client said, or
    /// guessed from the folder's top when it isn't gathered yet.
    fn guessed(&self, change: GatheredChange) -> Change {
        match change {
            GatheredChange::Add {
                place,
                path,
                holds,
                at,
            } => {
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
                    folder: GatheredFolder {
                        path,
                        holds,
                        name: None,
                    },
                    at: at.and_then(|at| usize::try_from(at).ok()),
                }
            }
            GatheredChange::Remove { place, path } => Change::Remove { place, path },
        }
    }
}

/// A client's change, with what an added folder holds settled.
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

/// The gathered folders file of the user of `home`.
fn file_of(home: &Home) -> PathBuf {
    config_watch::folder(&home.config).join(FILE)
}

/// The file's text: empty when it is missing, `None` (logged) when it
/// can't be read, is too big or isn't UTF-8, as TOML must be, so a save
/// never rewrites bytes it couldn't read.
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

/// The folders in `home`'s file, or `None` (logged) when it can't be read
/// or isn't TOML.
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

/// Writes a file with the usual folders for `home`, when there is none:
/// not even a link to nowhere, which is the user's.
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

/// Starts the watch on one gathered folder, which tells the clients in
/// `folders` when what it brings to its place may have changed; `None`
/// (logged) when it can't start. A folder that holds many is watched
/// itself, for a folder made, removed or renamed in it; a file saved there
/// is no change. A one-project folder is watched from its parent, for it
/// alone, so saving a file in the project is no change either. A folder
/// that doesn't exist yet is watched from the nearest folder above, and
/// counts as changed when it appears (see `config_watch`).
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

/// Whether `changed` in `folder` may have made, removed or renamed a
/// folder in it: a name that is a folder now, or is gone (it may have
/// been one).
fn changes_a_folder(folder: &Path, changed: &Changed) -> bool {
    match changed {
        Changed::All => true,
        Changed::Files(names) => names
            .iter()
            .any(|name| fs::metadata(folder.join(name)).map_or(true, |found| found.is_dir())),
    }
}

/// Starts the watch that reads the file again when it changes; `None`
/// (logged) when it can't start.
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
