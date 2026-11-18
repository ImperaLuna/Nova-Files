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

const WRITABLE: u32 = 0o700;
const PERMISSION_BITS: u32 = 0o777;
const PART: u8 = b'/';
pub const BUFFER_SIZE: usize = 65_536;
const NEW_FILE: OFlags = OFlags::WRONLY
    .union(OFlags::CREATE)
    .union(OFlags::EXCL)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
const ROOT: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::CLOEXEC);
pub const FOLDER: OFlags = ROOT.union(OFlags::NOFOLLOW);

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

#[derive(Debug, Clone, Copy)]
pub struct Stamp {
    pub mode: u32,
    pub modified: i64,
}

pub struct Unpacking {
    root: OwnedFd,
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

    pub fn hard_link(&self, parts: &[&[u8]], target: &[&[u8]]) -> Result<bool, WriteError> {
        let (Some((name, inside)), Some((target_name, target_inside))) =
            (parts.split_last(), target.split_last())
        else {
            return Ok(false);
        };
        if parts == target {
            return Ok(false);
        }
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
