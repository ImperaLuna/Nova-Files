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
//! read until what it found is kept, so a slow read never lands after a
//! newer one. Clients are told under the folders' own lock, in the order
//! they changed. A file that can't be read, or isn't TOML while the user
//! fixes it, keeps the folders last read: a passing error never empties a
//! place, and a change is never saved over it.
//!
//! A client's change goes to a writer thread (see `change_writer`), so the
//! core's loop never waits on the disk; it guesses what an added folder holds when the
//! client didn't say, edits the file's current text, saves it whole with
//! its mode (through a symlink, to where it points), then reads it again.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::thread;

use nova_files_api::{
    ClientId, Event, EventSink, Gathered, GatheredChange, GatheredFolder, Gathering, Holds,
};

use super::file::{self, Edit};
use super::{FILE, guess, usual_folders};
use crate::change_writer::ChangeWriter;
use crate::config_watch::{self, ConfigWatch};
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
    /// Set once the first client asked and the folders started loading.
    started: AtomicBool,
    /// Saves each change, on a thread of its own.
    writer: ChangeWriter<GatheredChange>,
    /// Stops with the store.
    watch: Arc<Mutex<Option<ConfigWatch>>>,
}

struct Shared {
    home: Option<Home>,
    /// Held from each read of the disk until what it found is kept, and
    /// across each save.
    disk: Mutex<()>,
    folders: Mutex<Folders>,
}

#[derive(Default)]
struct Folders {
    /// The file has been read once.
    loaded: bool,
    gathered: Arc<Gathered>,
    /// The clients that asked for the folders.
    listeners: BTreeMap<ClientId, Arc<dyn EventSink>>,
}

impl Store {
    /// The folders gathered by the user of `home`, read once a client
    /// asks. Without `home` both places stay empty.
    pub fn open(home: Option<Home>) -> Self {
        let shared = Arc::new(Shared {
            home,
            disk: Mutex::new(()),
            folders: Mutex::default(),
        });
        let saving = Arc::clone(&shared);
        Self {
            shared,
            started: AtomicBool::new(false),
            writer: ChangeWriter::new(SAVE_THREAD, WHAT, move |change| saving.save(change)),
            watch: Arc::default(),
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
        if !self.started.swap(true, Ordering::SeqCst) {
            self.load();
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

    /// Starts the watch, writes the usual folders when there is no file
    /// and reads it, on a thread of its own.
    fn load(&self) {
        let shared = Arc::clone(&self.shared);
        let watch = Arc::clone(&self.watch);
        let load = move || {
            // The watch goes in before the first read, so no change is
            // missed.
            if let Some(home) = &shared.home {
                *lock(&watch) = start_watch(home, &Arc::downgrade(&shared));
            }
            shared.first_read();
        };
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
    /// Writes the usual folders when there is no file, then reads it.
    fn first_read(&self) {
        let _disk = lock(&self.disk);
        if let Some(home) = &self.home {
            write_usual(home);
        }
        self.reload_holding_disk();
    }

    /// Reads the file again and tells the listening clients when the
    /// folders changed.
    fn reload(&self) {
        let _disk = lock(&self.disk);
        self.reload_holding_disk();
    }

    fn reload_holding_disk(&self) {
        // `None` keeps the folders as they are: the file can't be read.
        let read = self
            .home
            .as_ref()
            .map_or_else(|| Some(Gathered::default()), read_folders);
        let mut folders = lock(&self.folders);
        let first = !folders.loaded;
        folders.loaded = true;
        let gathered = match read {
            Some(gathered) => gathered,
            None if first => Gathered::default(),
            None => return,
        };
        if first || *folders.gathered != gathered {
            folders.gathered = Arc::new(gathered);
            let event = Event::Gathered(Arc::clone(&folders.gathered));
            folders
                .listeners
                .retain(|_, events| events.deliver(event.clone()).is_ok());
        }
    }

    /// Makes `change` in the file, then reads it again. A file that can't
    /// be read is left as it is. The first change of a new user writes the
    /// usual folders first, as the first read would.
    fn save(&self, change: GatheredChange) {
        let Some(home) = &self.home else {
            return;
        };
        // Guessed before the disk is held, since a folder on a hung mount
        // may take a while, and only for a folder not gathered yet.
        let change = self.guessed(change);
        let _disk = lock(&self.disk);
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
        // The watch sees the save too, later; reading now tells the
        // clients without waiting for it.
        self.reload_holding_disk();
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
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Ok(_) => return,
        Err(error) => {
            log::warn!("can't look at {}: {error}", path.display());
            return;
        }
    }
    let text = file::new_file(&home.folder, &usual_folders(home));
    if let Err(error) = private_file::replace(&path, text.as_bytes(), private_file::FILE_MODE) {
        log::warn!("can't write {}: {error}", path.display());
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
