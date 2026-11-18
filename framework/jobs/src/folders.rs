use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File, Metadata};
use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::{Path, PathBuf};

use rustix::fs::{CWD, Mode, OFlags, fstat, open, openat};
use rustix::io::{Errno, fcntl_dupfd_cloexec};

use super::plan::{Entry, Kind, Plan};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unreached {
    Changed,
    Failed(Errno),
}

impl From<Errno> for Unreached {
    fn from(errno: Errno) -> Self {
        match errno {
            Errno::LOOP | Errno::NOTDIR => Self::Changed,
            errno => Self::Failed(errno),
        }
    }
}

impl fmt::Display for Unreached {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Changed => f.write_str("a folder on the way changed since the plan"),
            Self::Failed(errno) => errno.fmt(f),
        }
    }
}

impl From<Unreached> for io::Error {
    fn from(unreached: Unreached) -> Self {
        match unreached {
            Unreached::Changed => Self::other(unreached.to_string()),
            Unreached::Failed(errno) => errno.into(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct At<'a> {
    pub folder: BorrowedFd<'a>,
    pub name: &'a Path,
    pub shown: &'a Path,
}

impl<'a> At<'a> {
    pub const fn path(path: &'a Path) -> Self {
        Self {
            folder: CWD,
            name: path,
            shown: path,
        }
    }

    pub fn folder_path(self) -> &'a Path {
        match self.name.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => Path::new("."),
        }
    }

    pub fn look(self) -> io::Result<Metadata> {
        let flags = OFlags::PATH.union(OFlags::NOFOLLOW).union(OFlags::CLOEXEC);
        File::from(openat(self.folder, self.name, flags, Mode::empty())?).metadata()
    }

    pub fn is_in(self, folder: &Path) -> bool {
        let opened = openat(
            self.folder,
            self.folder_path(),
            OFlags::PATH.union(OFlags::CLOEXEC),
            Mode::empty(),
        );
        match (opened.and_then(fstat), fs::metadata(folder)) {
            (Ok(open), Ok(there)) => Entry::of_stat(&open) == Entry::of(&there),
            _ => false,
        }
    }
}

pub struct Source {
    folder: OwnedFd,
    name: PathBuf,
    path: PathBuf,
}

impl Source {
    pub fn at(&self) -> At<'_> {
        At {
            folder: self.folder.as_fd(),
            name: &self.name,
            shown: &self.path,
        }
    }
}

#[derive(Default)]
pub struct Folders {
    planned: HashMap<PathBuf, Entry>,
    open: HashMap<PathBuf, OwnedFd>,
}

impl Folders {
    pub fn of(plan: &Plan) -> Self {
        Self {
            planned: plan
                .items
                .iter()
                .filter(|item| item.kind == Kind::Folder)
                .map(|item| (item.from.clone(), item.entry))
                .collect(),
            open: HashMap::new(),
        }
    }

    pub fn source(&mut self, path: &Path) -> Result<Source, Unreached> {
        let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
            return Err(Unreached::Failed(Errno::INVAL));
        };
        Ok(Source {
            folder: fcntl_dupfd_cloexec(self.open(folder)?, 0)?,
            name: PathBuf::from(name),
            path: path.to_path_buf(),
        })
    }

    pub fn planned_folder(&mut self, path: &Path) -> Result<Source, Unreached> {
        self.open(path)?;
        self.source(path)
    }

    pub fn open(&mut self, folder: &Path) -> Result<BorrowedFd<'_>, Unreached> {
        self.open.retain(|open, _| folder.starts_with(open));
        self.reach(folder)?;
        self.open
            .get(folder)
            .map(AsFd::as_fd)
            .ok_or(Unreached::Failed(Errno::NOENT))
    }

    fn reach(&mut self, folder: &Path) -> Result<(), Unreached> {
        if self.open.contains_key(folder) {
            return Ok(());
        }
        let flags = OFlags::PATH.union(OFlags::DIRECTORY).union(OFlags::CLOEXEC);
        let fd = match (
            self.planned.get(folder),
            folder.parent(),
            folder.file_name(),
        ) {
            (Some(&planned), Some(above), Some(name)) => {
                self.reach(above)?;
                let above = self.open.get(above).ok_or(Errno::NOENT)?;
                let fd = openat(above, name, flags.union(OFlags::NOFOLLOW), Mode::empty())?;
                if Entry::of_stat(&fstat(&fd)?) != planned {
                    return Err(Unreached::Changed);
                }
                fd
            }
            _ => open(folder, flags, Mode::empty())?,
        };
        self.open.insert(folder.to_path_buf(), fd);
        Ok(())
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn opened(&self) -> impl Iterator<Item = &Path> {
        self.open.keys().map(PathBuf::as_path)
    }
}

#[cfg(test)]
mod tests;
