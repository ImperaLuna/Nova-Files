//! Watches the core's config folder (`$XDG_CONFIG_HOME/nova-files`) and
//! says which of its files changed.
//!
//! The folder is watched, not each file, so a save of any kind is seen: an
//! editor writing in place (`CLOSE_WRITE`), or writing a temporary file and
//! renaming it over the old one (`MOVED_TO`), or the file being removed.
//! While the folder doesn't exist, the nearest folder above it that does is
//! watched instead, and the watch moves down as each folder below appears;
//! when the folder goes away, the watch moves back up. Each move reports
//! every file as changed, since the folder's files came or went with it. A
//! lost-events overflow does the same. While the folder is watched, the
//! folder above it is watched too, for the folder being replaced (a
//! symlinked config folder pointed somewhere else).
//!
//! A file the caller names that is a symlink is followed: the folder of the
//! file it points to is watched too, and a change there is reported under
//! the link's name. Only the last hop of a chain of links is followed.
//!
//! Saves come in bursts (a temporary file, a rename, an attribute change),
//! so the thread waits until the folder has been quiet for [`QUIET`], at
//! most [`LONGEST_WAIT`] after the first event, and then reports the burst
//! once. It sleeps in `poll` until the kernel has events or
//! [`ConfigWatch`] is dropped, which wakes it through an eventfd: no
//! polling loop.

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

/// The config folder's name, under the config home.
const FOLDER: &str = "nova-files";
const THREAD_NAME: &str = "nova-files-config";
/// How long the folder must stay quiet before a burst is reported.
pub const QUIET: Duration = Duration::from_millis(100);
/// The longest a burst is held back, however busy the folder stays.
const LONGEST_WAIT: Duration = Duration::from_secs(2);
/// Room for many events per read.
const READ_BUFFER: usize = 16_384;
/// What dropping [`ConfigWatch`] writes to the eventfd; any value wakes it.
const WAKE: u64 = 1;

/// What changes in the folder itself: its files saved, renamed or removed.
const IN_FOLDER: WatchFlags = WatchFlags::CREATE
    .union(WatchFlags::DELETE)
    .union(WatchFlags::MOVED_FROM)
    .union(WatchFlags::MOVED_TO)
    .union(WatchFlags::CLOSE_WRITE)
    .union(WatchFlags::DELETE_SELF)
    .union(WatchFlags::MOVE_SELF)
    .union(WatchFlags::ONLYDIR);
/// What matters in a folder above it: a folder below appearing, or this
/// one going away.
const ABOVE: WatchFlags = WatchFlags::CREATE
    .union(WatchFlags::MOVED_TO)
    .union(WatchFlags::DELETE_SELF)
    .union(WatchFlags::MOVE_SELF)
    .union(WatchFlags::ONLYDIR);
/// What matters in the folder above it while it is watched: it being
/// replaced, removed or renamed.
const PARENT: WatchFlags = WatchFlags::CREATE
    .union(WatchFlags::DELETE)
    .union(WatchFlags::MOVED_FROM)
    .union(WatchFlags::MOVED_TO)
    .union(WatchFlags::ONLYDIR);
/// What matters in a folder a linked file is in: as in the folder itself.
/// Added to what that folder is already watched for, since it may be the
/// folder itself or the one above.
const LINKED: WatchFlags = IN_FOLDER.union(WatchFlags::MASK_ADD);
/// The watched folder itself went away.
const GONE: ReadFlags = ReadFlags::DELETE_SELF
    .union(ReadFlags::MOVE_SELF)
    .union(ReadFlags::IGNORED);

/// Which of the folder's files changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Changed {
    /// These files, by name.
    Files(BTreeSet<OsString>),
    /// Any of them: the folder came or went, or events were lost.
    All,
}

impl Changed {
    /// Whether the file named `name` may have changed.
    pub fn touches(&self, name: &str) -> bool {
        self.touches_os(OsStr::new(name))
    }

    fn touches_os(&self, name: &OsStr) -> bool {
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

/// Watches the folder until dropped.
#[derive(Debug)]
pub struct ConfigWatch {
    stop: Arc<OwnedFd>,
    thread: Option<JoinHandle<()>>,
}

impl ConfigWatch {
    /// Watches `folder`, or the nearest folder above it while it doesn't
    /// exist, and calls `on_change` after each burst of changes. Of its
    /// files, those named in `files` are followed when they are symlinks.
    /// The watch
    /// is in place when this returns, so a file read after it is never
    /// newer than the changes reported.
    ///
    /// # Errors
    /// When inotify, the eventfd or the thread can't be set up, or no
    /// folder on the way up can be watched.
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
            // A panic in the thread was already reported where it happened.
            thread.join().ok();
        }
    }
}

/// The inotify instance and the one folder it watches now.
struct Watched {
    inotify: OwnedFd,
    /// The folder to watch.
    target: PathBuf,
    /// The files in it to follow when they are symlinks.
    files: Vec<OsString>,
    /// The watch in place, or none while nothing on the way up exists.
    current: Option<Armed>,
    /// Watches on the folders linked files are in: per watch, each file's
    /// name there and the link's name here.
    links: BTreeMap<i32, Vec<(OsString, OsString)>>,
}

/// The watch in place.
struct Armed {
    wd: i32,
    /// The folder it is on: the target, or a folder above it.
    folder: PathBuf,
    place: Place,
    /// On the target, the watch on the folder above it, when it could be
    /// made.
    parent: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    Target,
    Above,
}

impl Watched {
    /// Moves the watch to the target, or to the nearest folder above it
    /// that exists, and says where it went.
    fn arm(&mut self) -> io::Result<Place> {
        if let Some(old) = self.current.take() {
            // The kernel already dropped the watch of a folder that went.
            inotify::remove_watch(&self.inotify, old.wd).ok();
            if let Some(parent) = old.parent {
                inotify::remove_watch(&self.inotify, parent).ok();
            }
        }
        // A folder below can appear between failing to watch it and
        // watching its parent, unseen by either; then the walk starts
        // again. Each walk finds a deeper folder, so it ends.
        for _ in self.target.ancestors() {
            let armed = self.watch_nearest()?;
            let place = armed.place;
            // On the target, there is no folder below.
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

    /// A watch on the deepest folder of the target's path that exists.
    fn watch_nearest(&self) -> io::Result<Armed> {
        for folder in self.target.ancestors() {
            let (place, flags, parent) = if folder == self.target {
                // The folder above first: a swap after it is seen.
                let parent = folder
                    .parent()
                    .and_then(|parent| inotify::add_watch(&self.inotify, parent, PARENT).ok());
                (Place::Target, IN_FOLDER, parent)
            } else {
                // A watch on the folder above the target is the same watch
                // as `parent`, now with these flags.
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

    /// The folder one step below `folder` on the way to the target.
    fn next_below(&self, folder: &Path) -> Option<PathBuf> {
        self.target
            .ancestors()
            .find(|below| below.parent() == Some(folder))
            .map(Path::to_owned)
    }

    /// Watches the folder of each followed file that is a symlink, in
    /// place of the watches made before.
    fn relink(&mut self) {
        let (main, parent) = self
            .current
            .as_ref()
            .map_or((None, None), |armed| (Some(armed.wd), armed.parent));
        for wd in mem::take(&mut self.links).into_keys() {
            // The folder or the one above, watched for themselves too.
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
            // A link to a file not made yet still names where it will be.
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

    /// The followed files that `event`, on a linked file's folder, changed.
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

    /// What `events` changed, moving the watch first when the folder came
    /// or went.
    fn handle(&mut self, events: &[Event]) -> io::Result<Changed> {
        let Some(armed) = &self.current else {
            return Ok(Changed::All);
        };
        let (wd, place, parent) = (armed.wd, armed.place, armed.parent);
        let own_name = self.target.file_name().map(OsStr::to_owned);
        // Above the target, only the next folder down matters.
        let next = self
            .next_below(&armed.folder)
            .and_then(|below| below.file_name().map(OsStr::to_owned));
        let mut changed = Changed::Files(BTreeSet::new());
        let mut moved = false;
        for event in events {
            if event.mask.contains(ReadFlags::QUEUE_OVERFLOW) {
                // A lost event may have moved the folder: watch it afresh.
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
                // Above the target, `next` is always a name.
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
            // A followed file may have been replaced by another link.
            self.relink();
        }
        Ok(changed)
    }
}

/// One inotify event, copied out of the read buffer.
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
        // Waits for good while nothing is held back; with a burst held,
        // until the folder is quiet or the burst has waited long enough.
        let wait = first_event.map(|first| QUIET.min(LONGEST_WAIT.saturating_sub(first.elapsed())));
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
        let held_too_long = first_event.is_some_and(|first| first.elapsed() >= LONGEST_WAIT);
        // After a timeout there is nothing to read; the read says so at once.
        let events = read_events(&watched.inotify, &mut buffer)?;
        burst.add(watched.handle(&events)?);
        if !burst.is_empty() {
            first_event.get_or_insert_with(Instant::now);
        }
        // Only a held burst sets a timeout, so a quiet wake has one.
        if ready == 0 || held_too_long {
            on_change(&burst);
            burst = Changed::Files(BTreeSet::new());
            first_event = None;
        }
    }
}

/// Every event the kernel has queued, until a read would block.
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

/// The config folder under the config home `config`.
pub fn folder(config: &Path) -> PathBuf {
    config.join(FOLDER)
}

#[cfg(test)]
mod tests;
