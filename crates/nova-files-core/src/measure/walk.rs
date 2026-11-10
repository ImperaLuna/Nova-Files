//! Walking folder trees on a few threads at once, adding up what they hold.
//!
//! [`walk`] takes roots, each naming the tally slot its totals go to. Each
//! root (an absolute path) is looked at without following a link; a folder
//! root goes on a queue of folders that [`THREADS`] threads share. A thread
//! takes a folder, opens it through its parent's open handle (never through
//! a link, and never by a long path), and looks at each entry with
//! `statx`, asking only for the fields counted. Files go into the folder's
//! slot; folders go on the queue, except where another drive is mounted,
//! which counts as a folder but is never entered (see [`is_mount_root`]).
//! A folder, or a file with more than one hard link, counts once per walk,
//! however many roots or mounts reach it.
//!
//! An entry that vanished since its folder was read isn't there; any other
//! failure to read a folder or an entry counts it as unreadable, so the
//! totals say they miss something.
//!
//! The walk asks `stopped` before each read from the disk, and calls
//! `progress` each time a folder is done, and every [`REPORT_EVERY`]
//! entries of a big folder, so the caller can report.
//! It returns once the queue is empty and no thread is busy, or once
//! `stopped` says so.

use std::collections::HashSet;
use std::ffi::{CStr, CString};
use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;

use rustix::fs::{
    AtFlags, CWD, Dir, FileType, Mode, OFlags, Statx, StatxAttributes, StatxFlags, openat, statx,
};
use rustix::io::{Errno, fcntl_dupfd_cloexec};

use super::tally::Tally;

/// How many threads walk at once: enough to keep a fast drive busy,
/// few enough that a scan of all of home leaves the desktop alone.
pub const THREADS: usize = 4;

/// `statx` counts blocks of this many bytes, whatever the drive's own.
const BLOCK: u64 = 512;

const THREAD_NAME: &str = "nova-files-measure-walk";

/// How many entries of one folder a thread reads between two calls of
/// `progress`, so a big flat folder still reports as it goes.
const REPORT_EVERY: usize = 1000;

/// The fields a walk reads about each entry.
const WANTED: StatxFlags = StatxFlags::TYPE
    .union(StatxFlags::SIZE)
    .union(StatxFlags::BLOCKS)
    .union(StatxFlags::INO)
    .union(StatxFlags::NLINK);

/// Opening a folder to read it, never through a link.
const OPEN_FOLDER: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

/// One thing to measure, and the slot its totals go to.
pub struct Root {
    pub path: PathBuf,
    pub slot: usize,
}

/// A drive, as `statx` names it.
type Device = (u32, u32);

/// A folder waiting to be read.
struct Folder {
    at: Place,
    slot: usize,
    /// The drive it is on.
    device: Device,
}

/// Where a folder waiting to be read is.
enum Place {
    /// A root, by its absolute path.
    Root(PathBuf),
    /// `name` in the open folder `parent`. Folders queued from one parent
    /// share its handle, which closes once the last of them is read.
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

/// What every walking thread shares.
struct Walk<'a> {
    tally: &'a Tally,
    stopped: &'a (dyn Fn() -> bool + Sync),
    progress: &'a (dyn Fn() + Sync),
    queue: Mutex<Queue>,
    /// Signalled when a folder joins the queue, or the walk ends.
    ready: Condvar,
    /// Folders, and files with several hard links, already counted: drive
    /// and inode.
    seen: Mutex<HashSet<(Device, u64)>>,
}

#[derive(Default)]
struct Queue {
    folders: Vec<Folder>,
    /// Threads reading a folder now, which may add more.
    busy: usize,
}

/// Walks everything under `roots` into `tally`.
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
    for root in roots {
        if stopped() {
            return;
        }
        walk.root(root);
    }
    thread::scope(|scope| {
        for _ in 1..THREADS {
            // A thread that can't start leaves the work to the others; this
            // one always walks.
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
            Ok(stat) => self.count(root.slot, &stat, None, || Place::Root(root.path.clone())),
            Err(_) => self.tally.unreadable(root.slot),
        }
    }

    /// Takes folders off the queue and reads them until none are left.
    fn work(&self) {
        while let Some(folder) = self.next() {
            self.read(folder);
            (self.progress)();
            self.lock_queue().busy -= 1;
            // Those waiting see whether the walk is over, or stopped.
            self.ready.notify_all();
        }
    }

    /// The next folder to read, or `None` when the walk is over.
    fn next(&self) -> Option<Folder> {
        let mut queue = self.lock_queue();
        loop {
            if (self.stopped)() {
                // Wake the others, so they see it too.
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

    /// Counts everything in `folder`, queueing the folders inside it.
    fn read(&self, folder: Folder) {
        if (self.stopped)() {
            return;
        }
        let Ok(handle) = folder.at.open() else {
            self.tally.unreadable(folder.slot);
            return;
        };
        // Its own handle may close now: this one reads the folder.
        drop(folder.at);
        // Reads through a copy of the handle: opening the folder again
        // would need leave to enter it, which a folder that may only be
        // listed lacks.
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
                // The rest of the folder can't be read.
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

    /// Counts one entry into `slot`. A folder inside a folder on `device`
    /// (or any folder, for a root) joins the queue at the place `place`
    /// gives, unless another drive is mounted there.
    fn count(&self, slot: usize, stat: &Statx, device: Option<Device>, place: impl Fn() -> Place) {
        let own = (stat.stx_dev_major, stat.stx_dev_minor);
        let on_disk = stat.stx_blocks.saturating_mul(BLOCK);
        let folder = FileType::from_raw_mode(stat.stx_mode.into()) == FileType::Directory;
        if folder {
            let mount =
                |parent| is_mount_root(stat.stx_attributes_mask, stat.stx_attributes, own, parent);
            if device.is_some_and(mount) {
                // Another drive mounted here: its blocks aren't this one's.
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

    /// Whether this walk sees the folder or file `inode` on `device` for
    /// the first time.
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

/// Whether a folder on drive `own`, inside a folder on drive `parent`, is
/// where another filesystem is mounted. The kernel says so in the folder's
/// `statx` attributes (Linux 5.8+); a changed drive alone would also stop
/// at a btrfs subvolume, which is no mount, so it only answers on older
/// kernels, whose `mask` lacks the attribute.
fn is_mount_root(
    mask: StatxAttributes,
    attributes: StatxAttributes,
    own: Device,
    parent: Device,
) -> bool {
    if mask.contains(StatxAttributes::MOUNT_ROOT) {
        return attributes.contains(StatxAttributes::MOUNT_ROOT);
    }
    own != parent
}

/// Whether `name` is `.` or `..`, which every folder lists.
const fn is_dot(name: &CStr) -> bool {
    matches!(name.to_bytes(), b"." | b"..")
}

#[cfg(test)]
mod tests;
