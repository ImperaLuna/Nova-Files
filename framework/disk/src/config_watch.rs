use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::mem::MaybeUninit;
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use std::{fs, io, mem};

use rustix::event::{EventfdFlags, PollFd, PollFlags, Timespec, eventfd, poll};
use rustix::fs::inotify::{self, CreateFlags, ReadFlags, WatchFlags};
use rustix::io::Errno;

const FOLDER: &str = "nova-files";
const THREAD_NAME: &str = "nova-files-config";
pub const QUIET: Duration = Duration::from_millis(100);
const LONGEST_WAIT: Duration = Duration::from_secs(2);
const READ_BUFFER: usize = 16_384;
const WAKE: u64 = 1;

const IN_FOLDER: WatchFlags = WatchFlags::CREATE
    .union(WatchFlags::DELETE)
    .union(WatchFlags::MOVED_FROM)
    .union(WatchFlags::MOVED_TO)
    .union(WatchFlags::CLOSE_WRITE)
    .union(WatchFlags::DELETE_SELF)
    .union(WatchFlags::MOVE_SELF)
    .union(WatchFlags::ONLYDIR);
const ABOVE: WatchFlags = WatchFlags::CREATE
    .union(WatchFlags::MOVED_TO)
    .union(WatchFlags::DELETE_SELF)
    .union(WatchFlags::MOVE_SELF)
    .union(WatchFlags::ONLYDIR);
const PARENT: WatchFlags = WatchFlags::CREATE
    .union(WatchFlags::DELETE)
    .union(WatchFlags::MOVED_FROM)
    .union(WatchFlags::MOVED_TO)
    .union(WatchFlags::ONLYDIR);
const LINKED: WatchFlags = IN_FOLDER.union(WatchFlags::MASK_ADD);
const GONE: ReadFlags = ReadFlags::DELETE_SELF
    .union(ReadFlags::MOVE_SELF)
    .union(ReadFlags::IGNORED);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Changed {
    Files(BTreeSet<OsString>),
    All,
}

impl Changed {
    pub fn touches(&self, name: &str) -> bool {
        self.touches_os(OsStr::new(name))
    }

    pub fn touches_os(&self, name: &OsStr) -> bool {
        match self {
            Self::Files(names) => names.contains(name),
            Self::All => true,
        }
    }

    fn add(&mut self, more: Self) {
        match (self, more) {
            (Self::Files(names), Self::Files(more)) => names.extend(more),
            (all @ Self::Files(_), Self::All) => *all = Self::All,
            (Self::All, _) => {}
        }
    }

    fn is_empty(&self) -> bool {
        matches!(self, Self::Files(names) if names.is_empty())
    }
}

#[derive(Debug)]
pub struct ConfigWatch {
    stop: Arc<OwnedFd>,
    thread: Option<JoinHandle<()>>,
}

impl ConfigWatch {
    pub fn start(
        folder: PathBuf,
        files: &[&str],
        on_change: impl Fn(&Changed) + Send + 'static,
    ) -> io::Result<Self> {
        let inotify = inotify::init(CreateFlags::CLOEXEC.union(CreateFlags::NONBLOCK))?;
        let mut watched = Watched {
            inotify,
            target: folder,
            files: files.iter().map(OsString::from).collect(),
            current: None,
            links: BTreeMap::new(),
        };
        watched.arm()?;
        let stop = Arc::new(eventfd(0, EventfdFlags::CLOEXEC)?);
        let woken = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name(THREAD_NAME.to_owned())
            .spawn(move || {
                if let Err(error) = run(&mut watched, &woken, &on_change) {
                    log::warn!("stopped watching the config folder: {error}");
                }
            })?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for ConfigWatch {
    fn drop(&mut self) {
        let Some(thread) = self.thread.take() else {
            return;
        };
        if rustix::io::write(&*self.stop, &WAKE.to_ne_bytes()).is_ok() {
            thread.join().ok();
        }
    }
}

struct Watched {
    inotify: OwnedFd,
    target: PathBuf,
    files: Vec<OsString>,
    current: Option<Armed>,
    links: BTreeMap<i32, Vec<(OsString, OsString)>>,
}

struct Armed {
    wd: i32,
    folder: PathBuf,
    place: Place,
    parent: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    Target,
    Above,
}

impl Watched {
    fn arm(&mut self) -> io::Result<Place> {
        if let Some(old) = self.current.take() {
            // The kernel removes a watch itself when its folder goes (IN_IGNORED).
            // https://man7.org/linux/man-pages/man7/inotify.7.html
            inotify::remove_watch(&self.inotify, old.wd).ok();
            if let Some(parent) = old.parent {
                inotify::remove_watch(&self.inotify, parent).ok();
            }
        }
        for _ in self.target.ancestors() {
            let armed = self.watch_nearest()?;
            let place = armed.place;
            if self
                .next_below(&armed.folder)
                .is_some_and(|below| below.is_dir())
            {
                inotify::remove_watch(&self.inotify, armed.wd).ok();
                continue;
            }
            self.current = Some(armed);
            self.relink();
            return Ok(place);
        }
        let armed = self.watch_nearest()?;
        let place = armed.place;
        self.current = Some(armed);
        self.relink();
        Ok(place)
    }

    fn watch_nearest(&self) -> io::Result<Armed> {
        for folder in self.target.ancestors() {
            let (place, flags, parent) = if folder == self.target {
                let parent = folder
                    .parent()
                    .and_then(|parent| inotify::add_watch(&self.inotify, parent, PARENT).ok());
                (Place::Target, IN_FOLDER, parent)
            } else {
                (Place::Above, ABOVE, None)
            };
            if let Ok(wd) = inotify::add_watch(&self.inotify, folder, flags) {
                return Ok(Armed {
                    wd,
                    folder: folder.to_owned(),
                    place,
                    parent,
                });
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no folder above {} can be watched", self.target.display()),
        ))
    }

    fn next_below(&self, folder: &Path) -> Option<PathBuf> {
        self.target
            .ancestors()
            .find(|below| below.parent() == Some(folder))
            .map(Path::to_owned)
    }

    fn relink(&mut self) {
        let (main, parent) = self
            .current
            .as_ref()
            .map_or((None, None), |armed| (Some(armed.wd), armed.parent));
        for wd in mem::take(&mut self.links).into_keys() {
            if Some(wd) != main && Some(wd) != parent {
                inotify::remove_watch(&self.inotify, wd).ok();
            }
        }
        let Some(armed) = self
            .current
            .as_ref()
            .filter(|armed| armed.place == Place::Target)
        else {
            return;
        };
        for name in &self.files {
            let path = armed.folder.join(name);
            let Ok(link) = fs::read_link(&path) else {
                continue;
            };
            let real = fs::canonicalize(&path).unwrap_or_else(|_| armed.folder.join(link));
            let (Some(there), Some(there_name)) = (real.parent(), real.file_name()) else {
                continue;
            };
            match inotify::add_watch(&self.inotify, there, LINKED) {
                Ok(wd) => self
                    .links
                    .entry(wd)
                    .or_default()
                    .push((there_name.to_owned(), name.clone())),
                Err(error) => log::warn!("changes to {} won't be seen: {error}", real.display()),
            }
        }
    }

    fn linked(&self, event: &Event) -> Changed {
        let Some(names) = self.links.get(&event.wd) else {
            return Changed::Files(BTreeSet::new());
        };
        let gone = event.mask.intersects(GONE);
        Changed::Files(
            names
                .iter()
                .filter(|(there, _)| gone || event.name.as_ref() == Some(there))
                .map(|(_, here)| here.clone())
                .collect(),
        )
    }

    fn handle(&mut self, events: &[Event]) -> io::Result<Changed> {
        let Some(armed) = &self.current else {
            return Ok(Changed::All);
        };
        let (wd, place, parent) = (armed.wd, armed.place, armed.parent);
        let own_name = self.target.file_name().map(OsStr::to_owned);
        let next = self
            .next_below(&armed.folder)
            .and_then(|below| below.file_name().map(OsStr::to_owned));
        let mut changed = Changed::Files(BTreeSet::new());
        let mut moved = false;
        for event in events {
            if event.mask.contains(ReadFlags::QUEUE_OVERFLOW) {
                moved = true;
                changed.add(Changed::All);
            }
            changed.add(self.linked(event));
            if Some(event.wd) == parent {
                moved |= event.name == own_name;
                continue;
            }
            if event.wd != wd {
                continue;
            }
            if event.mask.intersects(GONE) {
                moved = true;
            } else if place == Place::Above {
                moved |= event.name == next;
            } else if let Some(name) = &event.name {
                changed.add(Changed::Files(BTreeSet::from([name.clone()])));
            }
        }
        if moved {
            let after = self.arm()?;
            if place == Place::Target || after == Place::Target {
                changed.add(Changed::All);
            }
        } else if self.files.iter().any(|name| changed.touches_os(name)) {
            self.relink();
        }
        Ok(changed)
    }
}

struct Event {
    wd: i32,
    mask: ReadFlags,
    name: Option<OsString>,
}

fn run(watched: &mut Watched, stop: &OwnedFd, on_change: &impl Fn(&Changed)) -> io::Result<()> {
    let mut buffer = vec![MaybeUninit::uninit(); READ_BUFFER];
    let mut burst = Changed::Files(BTreeSet::new());
    let mut first_event: Option<Instant> = None;
    loop {
        let wait = first_event.map(|first| wait_after(first.elapsed()));
        let timeout = wait.map(|wait| Timespec {
            tv_sec: i64::try_from(wait.as_secs()).unwrap_or(i64::MAX),
            tv_nsec: i64::from(wait.subsec_nanos()),
        });
        let mut fds = [
            PollFd::new(&watched.inotify, PollFlags::IN),
            PollFd::new(stop, PollFlags::IN),
        ];
        let ready = match poll(&mut fds, timeout.as_ref()) {
            Ok(ready) => ready,
            Err(Errno::INTR) => continue,
            Err(error) => return Err(error.into()),
        };
        if fds[1].revents().contains(PollFlags::IN) {
            return Ok(());
        }
        let held_too_long = first_event.is_some_and(|first| held_too_long(first.elapsed()));
        let events = read_events(&watched.inotify, &mut buffer)?;
        burst.add(watched.handle(&events)?);
        if !burst.is_empty() {
            first_event.get_or_insert_with(Instant::now);
        }
        if ready == 0 || held_too_long {
            on_change(&burst);
            burst = Changed::Files(BTreeSet::new());
            first_event = None;
        }
    }
}

fn wait_after(since_first: Duration) -> Duration {
    QUIET.min(LONGEST_WAIT.saturating_sub(since_first))
}

fn held_too_long(since_first: Duration) -> bool {
    since_first >= LONGEST_WAIT
}

fn read_events(fd: &OwnedFd, buffer: &mut [MaybeUninit<u8>]) -> io::Result<Vec<Event>> {
    let mut reader = inotify::Reader::new(fd, buffer);
    let mut events = Vec::new();
    loop {
        match reader.next() {
            Ok(event) => events.push(Event {
                wd: event.wd(),
                mask: event.events(),
                name: event
                    .file_name()
                    .map(|name| OsStr::from_bytes(name.to_bytes()).to_owned()),
            }),
            Err(Errno::AGAIN | Errno::INTR) => return Ok(events),
            Err(error) => return Err(error.into()),
        }
    }
}

pub fn folder(config: &Path) -> PathBuf {
    config.join(FOLDER)
}

#[cfg(test)]
mod tests;
