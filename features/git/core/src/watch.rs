use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rustix::event::{EventfdFlags, PollFd, PollFlags, Timespec, eventfd, poll};
use rustix::fs::inotify::{self, CreateFlags, ReadFlags, WatchFlags};
use rustix::io::Errno;

use crate::git_dir::{common_dir, git_dir};

const QUIET: Duration = Duration::from_millis(150);
const LONGEST_WAIT: Duration = Duration::from_secs(2);
const READ_BUFFER: usize = 16_384;
const WAKE: u64 = 1;
const HEADS: &str = "refs/heads";
const REMOTES: &str = "refs/remotes";
const CHANGES: WatchFlags = WatchFlags::CREATE
    .union(WatchFlags::DELETE)
    .union(WatchFlags::MOVED_FROM)
    .union(WatchFlags::MOVED_TO)
    .union(WatchFlags::CLOSE_WRITE)
    .union(WatchFlags::ONLYDIR);

#[derive(Debug, PartialEq, Eq)]
pub enum Woke {
    Folders(BTreeSet<usize>),
    All,
}

pub struct Stop {
    wake: OwnedFd,
}

impl Stop {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            wake: eventfd(0, EventfdFlags::CLOEXEC)?,
        })
    }

    pub fn stop(&self) {
        if let Err(error) = rustix::io::write(&self.wake, &WAKE.to_ne_bytes()) {
            log::warn!("could not stop a git watch: {error}");
        }
    }

    pub fn stopped(&self) -> bool {
        let mut fds = [PollFd::new(&self.wake, PollFlags::IN)];
        poll(&mut fds, Some(&Timespec::default())).is_ok_and(|ready| ready > 0)
    }
}

pub struct GitWatch {
    inotify: OwnedFd,
    folders: HashMap<i32, usize>,
}

impl GitWatch {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            inotify: inotify::init(CreateFlags::CLOEXEC.union(CreateFlags::NONBLOCK))?,
            folders: HashMap::new(),
        })
    }

    pub fn add(&mut self, index: usize, folder: &Path) {
        let Some(git_dir) = git_dir(folder) else {
            return;
        };
        let refs = common_dir(&git_dir).unwrap_or_else(|| git_dir.clone());
        let mut dirs = vec![git_dir, refs.join(HEADS)];
        dirs.extend(remotes(&refs.join(REMOTES)));
        for dir in dirs {
            match inotify::add_watch(&self.inotify, &dir, CHANGES) {
                Ok(wd) => {
                    self.folders.insert(wd, index);
                }
                Err(error) => log::debug!("no git watch on {}: {error}", dir.display()),
            }
        }
    }

    pub fn next(&self, stop: &Stop) -> io::Result<Option<Woke>> {
        let mut buffer = vec![MaybeUninit::uninit(); READ_BUFFER];
        let mut woke: Option<Woke> = None;
        let mut first: Option<Instant> = None;
        loop {
            let wait = first.map(|first| QUIET.min(LONGEST_WAIT.saturating_sub(first.elapsed())));
            let timeout = wait.map(|wait| Timespec {
                tv_sec: i64::try_from(wait.as_secs()).unwrap_or(i64::MAX),
                tv_nsec: i64::from(wait.subsec_nanos()),
            });
            let mut fds = [
                PollFd::new(&self.inotify, PollFlags::IN),
                PollFd::new(&stop.wake, PollFlags::IN),
            ];
            let ready = match poll(&mut fds, timeout.as_ref()) {
                Ok(ready) => ready,
                Err(Errno::INTR) => continue,
                Err(error) => return Err(error.into()),
            };
            if fds[1].revents().contains(PollFlags::IN) {
                return Ok(None);
            }
            if ready == 0 || first.is_some_and(|first| first.elapsed() >= LONGEST_WAIT) {
                return Ok(woke);
            }
            self.read(&mut buffer, &mut woke)?;
            if woke.is_some() {
                first.get_or_insert_with(Instant::now);
            }
        }
    }

    fn read(&self, buffer: &mut [MaybeUninit<u8>], woke: &mut Option<Woke>) -> io::Result<()> {
        let mut reader = inotify::Reader::new(&self.inotify, buffer);
        loop {
            match reader.next() {
                Ok(event) if event.events().contains(ReadFlags::QUEUE_OVERFLOW) => {
                    *woke = Some(Woke::All);
                }
                Ok(event) => {
                    let Some(&index) = self.folders.get(&event.wd()) else {
                        continue;
                    };
                    match woke {
                        Some(Woke::All) => {}
                        Some(Woke::Folders(folders)) => {
                            folders.insert(index);
                        }
                        None => *woke = Some(Woke::Folders(BTreeSet::from([index]))),
                    }
                }
                Err(Errno::AGAIN) => return Ok(()),
                Err(Errno::INTR) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
}

fn remotes(folder: &Path) -> Vec<PathBuf> {
    match fs::read_dir(folder) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.path())
            .collect(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => {
            log::debug!("{}: {error}", folder.display());
            Vec::new()
        }
    }
}
