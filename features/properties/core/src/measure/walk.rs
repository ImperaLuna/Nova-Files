use std::collections::HashSet;
use std::ffi::{CStr, CString};
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;

use rustix::fs::{AtFlags, CWD, Dir, FileType, Mode, OFlags, Statx, StatxFlags, openat, statx};
use rustix::io::{Errno, fcntl_dupfd_cloexec};

use super::tally::Tally;
pub use nova_files_disk::mount_root::Device;
use nova_files_disk::mount_root::is_mount_root;

pub const THREADS: usize = 4;

// statx counts stx_blocks in 512-byte units.
// https://man7.org/linux/man-pages/man2/statx.2.html
const BLOCK: u64 = 512;

const THREAD_NAME: &str = "nova-files-measure-walk";

const REPORT_EVERY: usize = 1000;

const WANTED: StatxFlags = StatxFlags::TYPE
    .union(StatxFlags::SIZE)
    .union(StatxFlags::BLOCKS)
    .union(StatxFlags::INO)
    .union(StatxFlags::NLINK);

const OPEN_FOLDER: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

pub struct Root {
    pub path: PathBuf,
    pub slot: usize,
    pub listed_in: Option<Device>,
}

pub fn device_of(folder: &Path) -> Option<Device> {
    statx(CWD, folder, AtFlags::SYMLINK_NOFOLLOW, StatxFlags::empty())
        .ok()
        .map(|stat| (stat.stx_dev_major, stat.stx_dev_minor))
}

struct Folder {
    at: Place,
    slot: usize,
    device: Device,
}

enum Place {
    Root(PathBuf),
    Inside { parent: Arc<OwnedFd>, name: CString },
}

impl Place {
    fn open(&self) -> rustix::io::Result<OwnedFd> {
        match self {
            Self::Root(path) => openat(CWD, path, OPEN_FOLDER, Mode::empty()),
            Self::Inside { parent, name } => {
                openat(&**parent, name.as_c_str(), OPEN_FOLDER, Mode::empty())
            }
        }
    }
}

struct Walk<'a> {
    tally: &'a Tally,
    stopped: &'a (dyn Fn() -> bool + Sync),
    progress: &'a (dyn Fn() + Sync),
    queue: Mutex<Queue>,
    ready: Condvar,
    seen: Mutex<HashSet<(Device, u64)>>,
}

#[derive(Default)]
struct Queue {
    folders: Vec<Folder>,
    busy: usize,
}

pub fn walk(
    roots: &[Root],
    tally: &Tally,
    stopped: &(dyn Fn() -> bool + Sync),
    progress: &(dyn Fn() + Sync),
) {
    let walk = Walk {
        tally,
        stopped,
        progress,
        queue: Mutex::default(),
        ready: Condvar::new(),
        seen: Mutex::default(),
    };
    for (counted, root) in roots.iter().enumerate() {
        if stopped() {
            return;
        }
        walk.root(root);
        if (counted + 1).is_multiple_of(REPORT_EVERY) {
            progress();
        }
    }
    thread::scope(|scope| {
        for _ in 1..THREADS {
            thread::Builder::new()
                .name(THREAD_NAME.to_owned())
                .spawn_scoped(scope, || walk.work())
                .ok();
        }
        walk.work();
    });
}

impl Walk<'_> {
    fn root(&self, root: &Root) {
        if !root.path.is_absolute() {
            self.tally.unreadable(root.slot);
            return;
        }
        match statx(CWD, &root.path, AtFlags::SYMLINK_NOFOLLOW, WANTED) {
            Ok(stat) => self.count(root.slot, &stat, root.listed_in, || {
                Place::Root(root.path.clone())
            }),
            Err(Errno::NOENT) if root.listed_in.is_some() => {}
            Err(_) => self.tally.unreadable(root.slot),
        }
    }

    fn work(&self) {
        while let Some(folder) = self.next() {
            self.read(folder);
            (self.progress)();
            self.lock_queue().busy -= 1;
            self.ready.notify_all();
        }
    }

    fn next(&self) -> Option<Folder> {
        let mut queue = self.lock_queue();
        loop {
            if (self.stopped)() {
                self.ready.notify_all();
                return None;
            }
            if let Some(folder) = queue.folders.pop() {
                queue.busy += 1;
                return Some(folder);
            }
            if queue.busy == 0 {
                return None;
            }
            queue = self
                .ready
                .wait(queue)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn read(&self, folder: Folder) {
        if (self.stopped)() {
            return;
        }
        let Ok(handle) = folder.at.open() else {
            self.tally.unreadable(folder.slot);
            return;
        };
        drop(folder.at);
        let Ok(mut entries) = fcntl_dupfd_cloexec(&handle, 0).and_then(Dir::new) else {
            self.tally.unreadable(folder.slot);
            return;
        };
        let handle = Arc::new(handle);
        let mut read = 0_usize;
        loop {
            if (self.stopped)() {
                return;
            }
            let Some(entry) = entries.next() else {
                return;
            };
            read += 1;
            if read.is_multiple_of(REPORT_EVERY) {
                (self.progress)();
            }
            let Ok(entry) = entry else {
                self.tally.unreadable(folder.slot);
                return;
            };
            let name = entry.file_name();
            if is_dot(name) {
                continue;
            }
            match statx(&*handle, name, AtFlags::SYMLINK_NOFOLLOW, WANTED) {
                Ok(stat) => {
                    let place = || Place::Inside {
                        parent: Arc::clone(&handle),
                        name: name.to_owned(),
                    };
                    self.count(folder.slot, &stat, Some(folder.device), place);
                }
                Err(Errno::NOENT) => {}
                Err(_) => self.tally.unreadable(folder.slot),
            }
        }
    }

    fn count(&self, slot: usize, stat: &Statx, device: Option<Device>, place: impl Fn() -> Place) {
        let own = (stat.stx_dev_major, stat.stx_dev_minor);
        let on_disk = stat.stx_blocks.saturating_mul(BLOCK);
        let folder = FileType::from_raw_mode(stat.stx_mode.into()) == FileType::Directory;
        if folder {
            let mount =
                |parent| is_mount_root(stat.stx_attributes_mask, stat.stx_attributes, own, parent);
            if device.is_some_and(mount) {
                self.tally.folder(slot, 0);
                return;
            }
            if !self.first_sight(own, stat.stx_ino) {
                return;
            }
            self.tally.folder(slot, on_disk);
            let folder = Folder {
                at: place(),
                slot,
                device: own,
            };
            self.lock_queue().folders.push(folder);
            self.ready.notify_one();
            return;
        }
        if stat.stx_nlink != 1 && !self.first_sight(own, stat.stx_ino) {
            return;
        }
        self.tally.file(slot, stat.stx_size, on_disk);
    }

    fn first_sight(&self, device: Device, inode: u64) -> bool {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert((device, inode))
    }

    fn lock_queue(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

const fn is_dot(name: &CStr) -> bool {
    matches!(name.to_bytes(), b"." | b"..")
}

#[cfg(test)]
mod tests;
