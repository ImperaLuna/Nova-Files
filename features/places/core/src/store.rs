use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::thread;

use nova_files_disk::change_writer::ChangeWriter;
use nova_files_disk::config_watch::{Changed, ConfigWatch};
use nova_files_disk::hand_edited::lock;
use nova_files_disk::home::Home;
use nova_files_disk::private_file::{self, Read};
use nova_files_messages::{ClientId, EventSink};
use nova_files_places_api::{Bookmark, BookmarkChange, Event, Place};

use super::bookmarks::{self, Lines};
use super::{USER_DIRS_FILE, places, usual_name};

const MAX_BYTES: usize = 1_048_576;
const LOAD_THREAD: &str = "nova-files-places";
const SAVE_THREAD: &str = "nova-files-bookmarks";
const WHAT: &str = "pinned folders";

pub struct Store {
    shared: Arc<Shared>,
    started: AtomicBool,
    writer: ChangeWriter<BookmarkChange>,
    watches: Arc<Mutex<Vec<ConfigWatch>>>,
}

struct Shared {
    home: Option<Home>,
    disk: Mutex<()>,
    lists: Mutex<Lists>,
}

#[derive(Default)]
struct Lists {
    loaded: bool,
    places: Vec<Place>,
    wanted: Vec<Place>,
    bookmarks: Arc<[Bookmark]>,
    listeners: BTreeMap<ClientId, Arc<dyn EventSink>>,
}

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
    pub fn open(home: Option<Home>) -> Self {
        let shared = Arc::new(Shared {
            home,
            disk: Mutex::new(()),
            lists: Mutex::default(),
        });
        let saving = Arc::clone(&shared);
        Self {
            shared,
            started: AtomicBool::new(false),
            writer: ChangeWriter::new(SAVE_THREAD, WHAT, move |change| saving.save(&change)),
            watches: Arc::default(),
        }
    }

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

    pub fn forget(&self, client: ClientId) {
        lock(&self.shared.lists).listeners.remove(&client);
    }

    pub fn change(&self, change: BookmarkChange) {
        if self.shared.home.is_some() {
            self.writer.send(change);
        }
    }

    fn load(&self) {
        let shared = Arc::clone(&self.shared);
        let watches = Arc::clone(&self.watches);
        let load = move || {
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
    fn reload(&self, which: Reread) {
        let _disk = lock(&self.disk);
        self.reload_holding_disk(which);
    }

    fn reload_holding_disk(&self, which: Reread) {
        let places = which.places().then(|| places(self.home.as_ref()));
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

    fn save(&self, change: &BookmarkChange) {
        let Some(home) = &self.home else {
            return;
        };
        let _disk = lock(&self.disk);
        let file = bookmarks_file(home);
        match private_file::saved_at(&file) {
            Ok(target) => {
                if let Some(mut lines) = read_lines(&target)
                    && lines.apply(change)
                {
                    let mode = private_file::mode_to_keep(&target);
                    if let Err(error) = private_file::replace(&target, &lines.to_bytes(), mode) {
                        log::warn!("can't save {}: {error}", target.display());
                    }
                }
            }
            Err(error) => log::warn!("can't save {}: {error}", file.display()),
        }
        self.reload_holding_disk(Reread::Bookmarks);
    }
}

fn tell(listeners: &mut BTreeMap<ClientId, Arc<dyn EventSink>>, event: &Event) {
    listeners.retain(|_, events| events.deliver(event.clone()).is_ok());
}

fn bookmarks_file(home: &Home) -> PathBuf {
    home.config.join(bookmarks::FOLDER).join(bookmarks::FILE)
}

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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::ffi::OsString;
    use std::fs;
    use std::sync::mpsc::{self, Receiver};

    use nova_files_test_folder::TestFolder;

    use super::*;

    struct Scratch(TestFolder);

    impl Scratch {
        fn new(name: &str) -> Self {
            let root = TestFolder::new(&format!("places-{name}"));
            fs::create_dir_all(root.join("config").join(bookmarks::FOLDER)).unwrap();
            Self(root)
        }

        fn home(&self) -> Home {
            Home {
                folder: self.0.path().to_path_buf(),
                config: self.0.join("config"),
                state: self.0.join("state"),
                data: self.0.join("share"),
                drive_trashes: false,
            }
        }

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
        let shared = Arc::new(shared);
        let saving = Arc::clone(&shared);
        let store = Store {
            shared,
            started: AtomicBool::new(true),
            writer: ChangeWriter::new(SAVE_THREAD, WHAT, move |change| saving.save(&change)),
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
        let home = scratch.0.path();
        assert!(shared.touches_a_place(home, &Changed::All));
        assert!(shared.touches_a_place(home, &names(&["notes.txt", "Documents"])));
        assert!(shared.touches_a_place(home, &names(&["Tunes"])));
        assert!(!shared.touches_a_place(home, &names(&["Clips"])));
        assert!(!shared.touches_a_place(home, &names(&["notes.txt", "deep"])));
        assert!(!shared.touches_a_place(home, &names(&[])));
    }
}
