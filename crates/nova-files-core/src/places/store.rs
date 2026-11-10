//! The places list as the core keeps it: the standard folders that exist
//! and the pinned folders, followed on disk and sent to the clients that
//! asked, each time either changes.
//!
//! Nothing is read until the first client asks, and then on a thread of
//! its own, so a slow Home (a folder on a hung network mount) never holds
//! up the core's loop, and a core that only serves a file picker reads
//! nothing. That thread starts three watches (see `config_watch`), then
//! reads both lists and sends them to every client that asked so far:
//! - the config folder, for `user-dirs.dirs`: the standard folders;
//! - the bookmarks file's folder: the pinned folders;
//! - Home, for a standard folder made or removed there. A standard folder
//!   `user-dirs.dirs` puts deeper in Home is only looked at again when one
//!   of the others changes.
//!
//! Every read of the disk and every save holds one lock, `disk`, from the
//! read until the lists it found are kept, so a slow read never lands
//! after a newer one. Clients are told under the lists' own lock, in the
//! order the lists changed. A bookmarks file that can't be read keeps the
//! pinned folders last read: a passing error never empties the list.
//!
//! A client's change goes to a writer thread, so the core's loop never
//! waits on the disk; it edits the bookmarks file's current bytes, saves it
//! whole with its mode (through a symlink, to where it points), then reads
//! the pinned folders again.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, Weak};
use std::thread;

use nova_files_api::{Bookmark, BookmarkChange, ClientId, Event, EventSink, Place};

use super::bookmarks::{self, Lines};
use super::{Home, USER_DIRS_FILE, places, usual_name};
use crate::config_watch::{Changed, ConfigWatch};
use crate::hand_edited::lock;
use crate::private_file::{self, Read};

/// A bookmarks file this big isn't one; it is neither read nor saved.
/// 1 MiB.
const MAX_BYTES: usize = 1_048_576;
const LOAD_THREAD: &str = "nova-files-places";
const SAVE_THREAD: &str = "nova-files-bookmarks";

/// The places list and who hears about it.
pub struct Store {
    shared: Arc<Shared>,
    /// Set once the first client asked and the lists started loading.
    started: AtomicBool,
    /// The writer's queue, started on the first change.
    writer: Mutex<Option<Sender<BookmarkChange>>>,
    /// Stop with the store.
    watches: Arc<Mutex<Vec<ConfigWatch>>>,
}

struct Shared {
    home: Option<Home>,
    /// Held from each read of the disk until what it found is kept, and
    /// across each save.
    disk: Mutex<()>,
    lists: Mutex<Lists>,
}

#[derive(Default)]
struct Lists {
    /// Both lists have been read once.
    loaded: bool,
    /// The standard folders that exist.
    places: Vec<Place>,
    /// Every standard folder, whether it exists or not.
    wanted: Vec<Place>,
    bookmarks: Arc<[Bookmark]>,
    /// The clients that asked for the places.
    listeners: BTreeMap<ClientId, Arc<dyn EventSink>>,
}

/// Which list to read again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reread {
    Places,
    Bookmarks,
    Both,
}

impl Reread {
    const fn places(self) -> bool {
        matches!(self, Self::Places | Self::Both)
    }

    const fn bookmarks(self) -> bool {
        matches!(self, Self::Bookmarks | Self::Both)
    }
}

impl Store {
    /// The places under `home`, read once a client asks. Without `home`
    /// both lists stay empty.
    pub fn open(home: Option<Home>) -> Self {
        Self {
            shared: Arc::new(Shared {
                home,
                disk: Mutex::new(()),
                lists: Mutex::default(),
            }),
            started: AtomicBool::new(false),
            writer: Mutex::new(None),
            watches: Arc::default(),
        }
    }

    /// Sends the places and pinned folders to `client` once they are read
    /// (now, when they already are), and each time they change.
    pub fn listen(&self, client: ClientId, events: &Arc<dyn EventSink>) {
        {
            let mut lists = lock(&self.shared.lists);
            let sent = if lists.loaded {
                events
                    .deliver(Event::Places(lists.places.clone()))
                    .and_then(|()| events.deliver(Event::Bookmarks(Arc::clone(&lists.bookmarks))))
            } else {
                Ok(())
            };
            if sent.is_ok() {
                lists.listeners.insert(client, Arc::clone(events));
            }
        }
        if !self.started.swap(true, Ordering::SeqCst) {
            self.load();
        }
    }

    /// Stops sending the places to `client`.
    pub fn forget(&self, client: ClientId) {
        lock(&self.shared.lists).listeners.remove(&client);
    }

    /// Makes `change` in the bookmarks file and tells the listening
    /// clients, on the writer's thread.
    pub fn change(&self, change: BookmarkChange) {
        if self.shared.home.is_none() {
            return;
        }
        let mut writer = lock(&self.writer);
        let change = match writer.as_ref() {
            Some(queue) => match queue.send(change) {
                Ok(()) => return,
                // The writer stopped: the change goes to a new one.
                Err(mpsc::SendError(change)) => change,
            },
            None => change,
        };
        *writer = start_writer(Arc::downgrade(&self.shared));
        if writer
            .as_ref()
            .is_some_and(|queue| queue.send(change).is_err())
        {
            log::warn!("the bookmarks writer stopped; pinned folders aren't saved");
            *writer = None;
        }
    }

    /// Starts the watches and reads both lists, on a thread of its own.
    fn load(&self) {
        let shared = Arc::clone(&self.shared);
        let watches = Arc::clone(&self.watches);
        let load = move || {
            // The watches go in before the first read, so no change is
            // missed.
            if let Some(home) = &shared.home {
                *lock(&watches) = watch(home, &Arc::downgrade(&shared));
            }
            shared.reload(Reread::Both);
        };
        let started = thread::Builder::new()
            .name(LOAD_THREAD.to_owned())
            .spawn(load.clone());
        if let Err(error) = started {
            log::warn!("can't read the places on their own thread, reading them here: {error}");
            load();
        }
    }
}

impl Shared {
    /// Reads `which` list again and tells the listening clients what
    /// changed; both lists, the first time.
    fn reload(&self, which: Reread) {
        let _disk = lock(&self.disk);
        self.reload_holding_disk(which);
    }

    fn reload_holding_disk(&self, which: Reread) {
        let places = which.places().then(|| places(self.home.as_ref()));
        // `None` keeps the pinned folders as they are: not asked for, or
        // the file can't be read.
        let bookmarks: Option<Arc<[Bookmark]>> = which
            .bookmarks()
            .then(|| {
                self.home.as_ref().map_or_else(
                    || Some(Arc::default()),
                    |home| read_lines(&bookmarks_file(home)).map(|lines| lines.bookmarks().into()),
                )
            })
            .flatten();
        let mut lists = lock(&self.lists);
        let first = !lists.loaded;
        lists.loaded = true;
        if let Some((places, wanted)) = places {
            lists.wanted = wanted;
            if first || lists.places != places {
                lists.places = places;
                let event = Event::Places(lists.places.clone());
                tell(&mut lists.listeners, &event);
            }
        }
        if let Some(bookmarks) = bookmarks
            && (first || lists.bookmarks != bookmarks)
        {
            lists.bookmarks = bookmarks;
            let event = Event::Bookmarks(Arc::clone(&lists.bookmarks));
            tell(&mut lists.listeners, &event);
        }
    }

    /// Whether `changed` in Home may have made or removed a standard
    /// folder: one with a usual name, or the name `user-dirs.dirs` gave it.
    fn touches_a_place(&self, home: &Path, changed: &Changed) -> bool {
        let names = match changed {
            Changed::All => return true,
            Changed::Files(names) => names,
        };
        let lists = lock(&self.lists);
        names.iter().filter_map(|name| name.to_str()).any(|name| {
            usual_name(name)
                || lists.wanted.iter().any(|place| {
                    place.path.parent() == Some(home)
                        && place.path.file_name() == Some(OsStr::new(name))
                })
        })
    }

    /// Makes `change` in the bookmarks file, then reads the pinned folders
    /// again. A file that can't be read is left as it is.
    fn save(&self, change: &BookmarkChange) {
        let Some(home) = &self.home else {
            return;
        };
        let _disk = lock(&self.disk);
        let target = private_file::saved_at(&bookmarks_file(home));
        if let Some(mut lines) = read_lines(&target)
            && lines.apply(change)
        {
            let mode = private_file::mode_to_keep(&target);
            if let Err(error) = private_file::replace(&target, &lines.to_bytes(), mode) {
                log::warn!("can't save {}: {error}", target.display());
            }
        }
        // The watch sees the save too, later; reading now tells the
        // clients without waiting for it.
        self.reload_holding_disk(Reread::Bookmarks);
    }
}

/// Sends `event` to every client in `listeners`, dropping those gone.
fn tell(listeners: &mut BTreeMap<ClientId, Arc<dyn EventSink>>, event: &Event) {
    listeners.retain(|_, events| events.deliver(event.clone()).is_ok());
}

/// The bookmarks file under `home`'s config folder.
fn bookmarks_file(home: &Home) -> PathBuf {
    home.config.join(bookmarks::FOLDER).join(bookmarks::FILE)
}

/// The bookmarks file's lines: none when it is missing, `None` (logged)
/// when it can't be read or is too big.
fn read_lines(path: &Path) -> Option<Lines> {
    match private_file::read(path, MAX_BYTES) {
        Read::Bytes(bytes) => Some(Lines::parse(&bytes)),
        Read::Missing => Some(Lines::default()),
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

/// Starts the watches that read the lists again: the config folder, the
/// bookmarks file's folder and Home. One that can't start is logged.
fn watch(home: &Home, shared: &Weak<Shared>) -> Vec<ConfigWatch> {
    let config = {
        let shared = Weak::clone(shared);
        ConfigWatch::start(home.config.clone(), &[USER_DIRS_FILE], move |changed| {
            if changed.touches(USER_DIRS_FILE)
                && let Some(shared) = shared.upgrade()
            {
                shared.reload(Reread::Places);
            }
        })
    };
    let bookmarks = {
        let shared = Weak::clone(shared);
        ConfigWatch::start(
            home.config.join(bookmarks::FOLDER),
            &[bookmarks::FILE],
            move |changed| {
                if changed.touches(bookmarks::FILE)
                    && let Some(shared) = shared.upgrade()
                {
                    shared.reload(Reread::Bookmarks);
                }
            },
        )
    };
    let folders = {
        let shared = Weak::clone(shared);
        let folder = home.folder.clone();
        ConfigWatch::start(home.folder.clone(), &[], move |changed| {
            if let Some(shared) = shared.upgrade()
                && shared.touches_a_place(&folder, changed)
            {
                shared.reload(Reread::Places);
            }
        })
    };
    [config, bookmarks, folders]
        .into_iter()
        .filter_map(|started| {
            started
                .map_err(|error| log::warn!("places changed on disk won't be seen: {error}"))
                .ok()
        })
        .collect()
}

/// Starts the thread that makes each change it is sent, in order, or none
/// when no thread can start.
fn start_writer(shared: Weak<Shared>) -> Option<Sender<BookmarkChange>> {
    let (queue, changes) = mpsc::channel::<BookmarkChange>();
    let started = thread::Builder::new()
        .name(SAVE_THREAD.to_owned())
        .spawn(move || {
            for change in changes {
                let Some(shared) = shared.upgrade() else {
                    return;
                };
                shared.save(&change);
            }
        });
    match started {
        Ok(_) => Some(queue),
        Err(error) => {
            log::warn!("can't start the bookmarks writer; pinned folders aren't saved: {error}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::ffi::OsString;
    use std::fs;
    use std::sync::mpsc::Receiver;

    use super::*;

    /// A home with its config folder, removed when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("nova-places-{name}-{}", std::process::id()));
            fs::remove_dir_all(&root).ok();
            fs::create_dir_all(root.join("config").join(bookmarks::FOLDER)).unwrap();
            Self(root)
        }

        fn home(&self) -> Home {
            Home {
                folder: self.0.clone(),
                config: self.0.join("config"),
                state: self.0.join("state"),
                data: self.0.join("share"),
            }
        }

        /// Shared lists already read once, and a client listening to them.
        fn listening(&self) -> (Shared, Receiver<Event>) {
            let shared = Shared {
                home: Some(self.home()),
                disk: Mutex::new(()),
                lists: Mutex::default(),
            };
            shared.reload(Reread::Both);
            let (sink, events) = mpsc::channel();
            lock(&shared.lists)
                .listeners
                .insert(ClientId(1), Arc::new(sink));
            (shared, events)
        }

        fn pin(&self, path: &str) {
            fs::write(bookmarks_file(&self.home()), format!("file://{path}\n")).unwrap();
        }

        fn make_documents(&self) {
            fs::create_dir(self.0.join("Documents")).unwrap();
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    fn names(names: &[&str]) -> Changed {
        Changed::Files(names.iter().map(OsString::from).collect::<BTreeSet<_>>())
    }

    #[test]
    fn rereading_one_list_leaves_the_other_as_it_was() {
        let scratch = Scratch::new("reread");
        let (shared, events) = scratch.listening();
        scratch.make_documents();
        scratch.pin("/pinned");

        shared.reload(Reread::Bookmarks);
        assert!(matches!(events.try_recv(), Ok(Event::Bookmarks(_))));
        assert!(events.try_recv().is_err(), "the places weren't read");
        scratch.pin("/pinned-again");
        shared.reload(Reread::Places);
        assert!(matches!(events.try_recv(), Ok(Event::Places(_))));
        assert!(events.try_recv().is_err(), "the pins weren't read again");
    }

    #[test]
    fn a_forgotten_client_hears_nothing_more() {
        let scratch = Scratch::new("forget");
        let (shared, events) = scratch.listening();
        let store = Store {
            shared: Arc::new(shared),
            started: AtomicBool::new(true),
            writer: Mutex::new(None),
            watches: Arc::default(),
        };
        store.forget(ClientId(1));
        scratch.pin("/pinned");
        store.shared.reload(Reread::Both);
        assert!(events.try_recv().is_err());
    }

    #[test]
    fn only_a_standard_folders_name_in_home_touches_a_place() {
        let scratch = Scratch::new("touches");
        fs::write(
            scratch.0.join("config").join(USER_DIRS_FILE),
            "XDG_MUSIC_DIR=\"$HOME/Tunes\"\nXDG_VIDEOS_DIR=\"$HOME/deep/Clips\"\n",
        )
        .unwrap();
        let (shared, _events) = scratch.listening();
        let home = scratch.0.as_path();
        assert!(shared.touches_a_place(home, &Changed::All));
        assert!(shared.touches_a_place(home, &names(&["notes.txt", "Documents"])));
        assert!(shared.touches_a_place(home, &names(&["Tunes"])));
        // Clips is wanted, but deeper than Home, so a `Clips` here isn't it.
        assert!(!shared.touches_a_place(home, &names(&["Clips"])));
        assert!(!shared.touches_a_place(home, &names(&["notes.txt", "deep"])));
        assert!(!shared.touches_a_place(home, &names(&[])));
    }
}
