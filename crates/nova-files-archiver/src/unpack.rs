//! Writes entries into the folder the helper unpacks into, and nowhere
//! else. Every folder on the way is opened one part at a time, never
//! following a link, so a link an archive made earlier can't lead a later
//! entry out. A name already there is replaced, unless it is a folder,
//! which stays and keeps the later entry out: the later of two entries
//! with one name wins, as `tar` does.

use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::ffi::OsStrExt as _;

use rustix::fs::{
    self, AtFlags, Mode, OFlags, Timespec, Timestamps, UTIME_OMIT, linkat, mkdirat, openat,
    symlinkat, unlinkat,
};
use rustix::io::Errno;

/// What a folder needs while the helper writes inside it; its own
/// permissions go on once everything is in.
const WRITABLE: u32 = 0o700;
const PERMISSION_BITS: u32 = 0o777;
const PART: u8 = b'/';
const BUFFER_SIZE: usize = 65_536;
const NEW_FILE: OFlags = OFlags::WRONLY
    .union(OFlags::CREATE)
    .union(OFlags::EXCL)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
/// The unpack folder itself, opened again.
const ROOT: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::CLOEXEC);
/// A folder inside it, never through a link.
pub const FOLDER: OFlags = ROOT.union(OFlags::NOFOLLOW);

/// A write that failed, apart from reading the archive.
#[derive(Debug)]
pub struct WriteError(pub io::Error);

impl From<Errno> for WriteError {
    fn from(errno: Errno) -> Self {
        Self(errno.into())
    }
}

impl From<io::Error> for WriteError {
    fn from(error: io::Error) -> Self {
        Self(error)
    }
}

/// The parts of `path`, or `None` when one is `..`. Empty parts and `.`
/// are left out.
pub fn parts(path: &[u8]) -> Option<Vec<&[u8]>> {
    let mut parts = Vec::new();
    for part in path.split(|&byte| byte == PART) {
        match part {
            b"" | b"." => {}
            b".." => return None,
            part => parts.push(part),
        }
    }
    Some(parts)
}

/// When and with which permissions an entry was last changed.
#[derive(Debug, Clone, Copy)]
pub struct Stamp {
    pub mode: u32,
    /// Seconds since 1970, UTC.
    pub modified: i64,
}

/// The folder being unpacked into, and the folders made inside it, whose
/// permissions and times go on last.
pub struct Unpacking {
    root: OwnedFd,
    /// The permission bits the user's umask leaves.
    keep: u32,
    folders: Vec<(Vec<Vec<u8>>, Stamp)>,
}

impl Unpacking {
    pub const fn new(root: OwnedFd, umask: u32) -> Self {
        Self {
            root,
            keep: PERMISSION_BITS & !umask,
            folders: Vec::new(),
        }
    }

    /// Makes the folder `parts`, and those it is in.
    pub fn folder(&mut self, parts: &[&[u8]], stamp: Stamp) -> Result<(), WriteError> {
        let Some((name, inside)) = parts.split_last() else {
            return Ok(());
        };
        let parent = self.open(inside, true)?;
        make_folder(&parent, name)?;
        self.folders
            .push((parts.iter().map(|part| part.to_vec()).collect(), stamp));
        Ok(())
    }

    /// Writes the file `parts` with what `contents` holds, and says how
    /// many bytes that was. A read that fails comes back as an `Ok` error
    /// inside, so it tells apart from a write.
    pub fn file(
        &self,
        parts: &[&[u8]],
        stamp: Stamp,
        contents: &mut impl Read,
    ) -> Result<io::Result<u64>, WriteError> {
        let Some((name, inside)) = parts.split_last() else {
            return Ok(Ok(0));
        };
        let parent = self.open(inside, true)?;
        let made = replacing(&parent, name, || {
            openat(&parent, os(name), NEW_FILE, Mode::from_raw_mode(WRITABLE))
        });
        let mut file = match made {
            Ok(fd) => File::from(fd),
            Err(Errno::ISDIR) => return Ok(Ok(0)),
            Err(errno) => return Err(errno.into()),
        };
        let written = match copy(contents, &mut file) {
            Copied::Done(written) => written,
            Copied::Read(error) => return Ok(Err(error)),
            Copied::Write(error) => return Err(error.into()),
        };
        fs::fchmod(&file, Mode::from_raw_mode(stamp.mode & self.keep))?;
        fs::futimens(&file, &times(stamp.modified))?;
        Ok(Ok(written))
    }

    /// Makes `parts` a link to `target`, which is never followed here.
    pub fn link(&self, parts: &[&[u8]], target: &[u8]) -> Result<(), WriteError> {
        let Some((name, inside)) = parts.split_last() else {
            return Ok(());
        };
        let parent = self.open(inside, true)?;
        match replacing(&parent, name, || symlinkat(os(target), &parent, os(name))) {
            Ok(()) | Err(Errno::ISDIR) => Ok(()),
            Err(errno) => Err(errno.into()),
        }
    }

    /// Makes `parts` another name for the file `target` already unpacked;
    /// `false` when there is no such file.
    pub fn hard_link(&self, parts: &[&[u8]], target: &[&[u8]]) -> Result<bool, WriteError> {
        let (Some((name, inside)), Some((target_name, target_inside))) =
            (parts.split_last(), target.split_last())
        else {
            return Ok(false);
        };
        let Ok(from) = self.open(target_inside, false) else {
            return Ok(false);
        };
        let parent = self.open(inside, true)?;
        match replacing(&parent, name, || {
            linkat(&from, os(target_name), &parent, os(name), AtFlags::empty())
        }) {
            Ok(()) => Ok(true),
            Err(Errno::NOENT | Errno::ISDIR) => Ok(false),
            Err(errno) => Err(errno.into()),
        }
    }

    /// Gives each folder made its own permissions and time, those inside
    /// first, so a folder's time isn't changed by what goes in after.
    pub fn finish(mut self) -> Result<(), WriteError> {
        self.folders
            .sort_by_key(|(parts, _)| std::cmp::Reverse(parts.len()));
        for (parts, stamp) in &self.folders {
            let parts: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
            let folder = self.open(&parts, false)?;
            fs::fchmod(&folder, Mode::from_raw_mode(stamp.mode & self.keep))?;
            fs::futimens(&folder, &times(stamp.modified))?;
        }
        Ok(())
    }

    /// Opens the folder `parts` inside the root, one part at a time and
    /// never through a link; with `make`, makes the parts not there.
    fn open(&self, parts: &[&[u8]], make: bool) -> Result<OwnedFd, Errno> {
        let mut folder = openat(&self.root, c".", ROOT, Mode::empty())?;
        for part in parts {
            if make {
                make_folder(&folder, part)?;
            }
            folder = open_folder(&folder, part)?;
        }
        Ok(folder)
    }
}

fn open_folder(parent: &impl AsFd, name: &[u8]) -> Result<OwnedFd, Errno> {
    openat(parent, os(name), FOLDER, Mode::empty())
}

/// Makes the folder `name` in `parent`; one already there stays, and
/// anything else there gives way.
fn make_folder(parent: &impl AsFd, name: &[u8]) -> Result<(), Errno> {
    match mkdirat(parent, os(name), Mode::from_raw_mode(WRITABLE)) {
        Err(Errno::EXIST) if open_folder(parent, name).is_err() => {
            unlinkat(parent, os(name), AtFlags::empty())?;
            mkdirat(parent, os(name), Mode::from_raw_mode(WRITABLE))
        }
        Err(Errno::EXIST) => Ok(()),
        made => made,
    }
}

/// Runs `make`, which makes `name` in `parent`; when the name is taken by
/// anything but a folder, that goes and `make` runs again. A folder there
/// stays, and the answer is `ISDIR`.
fn replacing<T>(
    parent: &impl AsFd,
    name: &[u8],
    mut make: impl FnMut() -> Result<T, Errno>,
) -> Result<T, Errno> {
    match make() {
        Err(Errno::EXIST) => {
            unlinkat(parent, os(name), AtFlags::empty())?;
            make()
        }
        result => result,
    }
}

enum Copied {
    Done(u64),
    Read(io::Error),
    Write(io::Error),
}

fn copy(from: &mut impl Read, to: &mut File) -> Copied {
    use std::io::Write as _;
    let mut buffer = vec![0; BUFFER_SIZE];
    let mut written = 0;
    loop {
        let read = match from.read(&mut buffer) {
            Ok(0) => return Copied::Done(written),
            Ok(read) => read,
            Err(error) => return Copied::Read(error),
        };
        if let Err(error) = to.write_all(&buffer[..read]) {
            return Copied::Write(error);
        }
        written += read as u64;
    }
}

const fn times(modified: i64) -> Timestamps {
    Timestamps {
        last_access: Timespec {
            tv_sec: 0,
            tv_nsec: UTIME_OMIT,
        },
        last_modification: Timespec {
            tv_sec: modified,
            tv_nsec: 0,
        },
    }
}

fn os(bytes: &[u8]) -> &OsStr {
    OsStr::from_bytes(bytes)
}
