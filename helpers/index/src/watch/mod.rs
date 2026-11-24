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
const READ_BUFFER: usize = 65_536;
const WAKE: u64 = 1;

#[derive(Debug)]
pub struct Watcher {
    stop: Arc<OwnedFd>,
    thread: Option<JoinHandle<io::Result<()>>>,
}

impl Watcher {
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
        index.release_details()?;
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

    pub fn stop(mut self) -> io::Result<()> {
        self.finish()
    }

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

fn is_readable(fd: &OwnedFd) -> bool {
    let mut fds = [PollFd::new(fd, PollFlags::IN)];
    let now = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    poll(&mut fds, Some(&now)).is_ok_and(|ready| ready > 0)
}

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
