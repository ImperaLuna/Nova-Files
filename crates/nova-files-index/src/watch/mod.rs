//! The watcher: follows every indexed folder with inotify and appends what
//! changes to the change log.
//!
//! [`Watcher::start`] installs a watch on each folder the index holds,
//! then catches up with what changed since the crawl (`folders::catch_up`),
//! before it returns: a change made after the watches went in is never
//! missed, and one made before is found by the catch-up. Its thread then sleeps in `poll` until the kernel has events or
//! [`Watcher::stop`] wakes it through an eventfd: no polling loop. Each
//! batch of events becomes [`Change`]s (`folders` decides which), written
//! to the log before `on_change` is called, so a reader woken by it finds
//! them there.
//!
//! inotify watches one folder at a time without root. Since Linux 5.11 the
//! watch limit grows with memory, up to 1,048,576, far past a home folder.
//! A folder that can't be watched (over the limit, or not readable) gets a
//! [`Change::Rescan`]: its changes won't be seen until it is read again.

mod folders;

use std::ffi::OsStr;
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStrExt as _;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use rustix::event::{EventfdFlags, PollFd, PollFlags, Timespec, eventfd, poll};
use rustix::fs::inotify::{self, CreateFlags};
use rustix::io::Errno;

use crate::changes::{Change, ChangeLog};
use crate::reader::Index;
use folders::{Event, Folders};

const THREAD_NAME: &str = "nova-files-watch";
/// Room for a few hundred events per read.
const READ_BUFFER: usize = 65_536;
/// What [`Watcher::stop`] writes to the eventfd; any value wakes the poll.
const WAKE: u64 = 1;

/// Follows the indexed folders until stopped or dropped.
#[derive(Debug)]
pub struct Watcher {
    stop: Arc<OwnedFd>,
    thread: Option<JoinHandle<io::Result<()>>>,
}

impl Watcher {
    /// Watches every folder in `index`, catches up with what changed
    /// since it was crawled, then appends changes to `log` and calls
    /// `on_change` after each batch, the catch-up's included.
    ///
    /// # Errors
    /// When inotify, the eventfd or the thread can't be set up.
    pub fn start(
        index: &Index,
        mut log: ChangeLog,
        on_change: impl Fn() + Send + 'static,
    ) -> io::Result<Self> {
        let inotify = inotify::init(CreateFlags::CLOEXEC.union(CreateFlags::NONBLOCK))?;
        let stop = Arc::new(eventfd(0, EventfdFlags::CLOEXEC)?);
        let asked = Arc::clone(&stop);
        let stopping = Box::new(move || is_readable(&asked));
        let mut folders = Folders::new(inotify, index.root().to_owned(), stopping);
        let mut caught = folders.watch_index(index);
        caught.extend(folders.catch_up(index));
        for change in &caught {
            log.append(change)?;
        }
        if !caught.is_empty() {
            on_change();
        }
        let woken = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name(THREAD_NAME.to_owned())
            .spawn(move || run(&mut folders, &woken, &mut log, &on_change))?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }

    /// Stops the thread and waits for it.
    ///
    /// # Errors
    /// The error that ended the thread early, for example a full disk
    /// while writing the log.
    pub fn stop(mut self) -> io::Result<()> {
        self.finish()
    }

    /// Whether the thread still follows the folders. It ends early only
    /// on an error, such as a full disk while writing the log; changes
    /// after that are missed until the index is crawled anew.
    pub fn is_running(&self) -> bool {
        self.thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
    }

    fn finish(&mut self) -> io::Result<()> {
        let Some(thread) = self.thread.take() else {
            return Ok(());
        };
        rustix::io::write(&*self.stop, &WAKE.to_ne_bytes())?;
        thread
            .join()
            .unwrap_or_else(|_| Err(io::Error::other("the watcher thread panicked")))
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // Nobody is left to hear about an error.
        self.finish().ok();
    }
}

fn run(
    folders: &mut Folders,
    stop: &OwnedFd,
    log: &mut ChangeLog,
    on_change: &impl Fn(),
) -> io::Result<()> {
    let mut buffer = vec![MaybeUninit::uninit(); READ_BUFFER];
    loop {
        let mut fds = [
            PollFd::new(folders.fd(), PollFlags::IN),
            PollFd::new(stop, PollFlags::IN),
        ];
        match poll(&mut fds, None) {
            Ok(_) | Err(Errno::INTR) => {}
            Err(error) => return Err(error.into()),
        }
        if fds[1].revents().contains(PollFlags::IN) {
            return Ok(());
        }
        let events = read_events(folders.fd(), &mut buffer)?;
        let changes: Vec<Change> = folders.handle(&events);
        for change in &changes {
            log.append(change)?;
        }
        if !changes.is_empty() {
            on_change();
        }
    }
}

/// Whether `fd` has something to read now, without waiting.
fn is_readable(fd: &OwnedFd) -> bool {
    let mut fds = [PollFd::new(fd, PollFlags::IN)];
    let now = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    poll(&mut fds, Some(&now)).is_ok_and(|ready| ready > 0)
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
                cookie: event.cookie(),
                name: event
                    .file_name()
                    .map(|name| OsStr::from_bytes(name.to_bytes()).to_owned()),
            }),
            Err(Errno::AGAIN | Errno::INTR) => return Ok(events),
            Err(error) => return Err(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_counter_is_readable_only_once_written_to() {
        let counter = eventfd(0, EventfdFlags::CLOEXEC).unwrap();
        assert!(!is_readable(&counter));
        rustix::io::write(&counter, &WAKE.to_ne_bytes()).unwrap();
        assert!(is_readable(&counter));
    }
}
