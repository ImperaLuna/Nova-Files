//! Reaching a job's planned items through their folders, without leaving
//! the trees the user chose.
//!
//! Each folder the plan walked is opened from the folder above it without
//! following a link, and must still be the folder planned (same device
//! and inode). So a folder swapped for a link, or for a look-alike, after
//! the plan (while the job waits on a question, say) can't lead the job
//! anywhere else: what it reaches through is refused instead. Any other
//! folder, where the user's paths start, is reached as the user named
//! it. Only the folders on the way to the one last reached stay open, so
//! a job across many folders never holds more than one path's worth.

use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File, Metadata};
use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::{Path, PathBuf};

use rustix::fs::{CWD, Mode, OFlags, fstat, open, openat};
use rustix::io::{Errno, fcntl_dupfd_cloexec};

use super::plan::{Entry, Kind, Plan};

/// Why an entry couldn't be reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unreached {
    /// A folder on the way changed since the plan: it became a link, a
    /// file, or another folder.
    Changed,
    Failed(Errno),
}

impl From<Errno> for Unreached {
    /// A link or a file where a folder was planned is a change.
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

/// An entry named `name` in the open folder `folder`; or, with the
/// working folder, a path named in full. `shown` is its whole path, for
/// messages.
#[derive(Clone, Copy)]
pub struct At<'a> {
    pub folder: BorrowedFd<'a>,
    pub name: &'a Path,
    pub shown: &'a Path,
}

impl<'a> At<'a> {
    /// `path`, reached as named.
    pub const fn path(path: &'a Path) -> Self {
        Self {
            folder: CWD,
            name: path,
            shown: path,
        }
    }

    /// The folder the entry is in, as a path from `folder`.
    pub fn folder_path(self) -> &'a Path {
        match self.name.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => Path::new("."),
        }
    }

    /// What is there now, a link not followed.
    ///
    /// # Errors
    /// When nothing is, or it can't be looked at.
    pub fn look(self) -> io::Result<Metadata> {
        let flags = OFlags::PATH.union(OFlags::NOFOLLOW).union(OFlags::CLOEXEC);
        File::from(openat(self.folder, self.name, flags, Mode::empty())?).metadata()
    }

    /// Whether the entry is in `folder`.
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

/// An entry reached through its folder, the folder held open.
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

/// The open folders of a job's plan.
#[derive(Default)]
pub struct Folders {
    /// Which entry each folder of the plan was.
    planned: HashMap<PathBuf, Entry>,
    open: HashMap<PathBuf, OwnedFd>,
}

impl Folders {
    /// The folders of `plan`.
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

    /// `path`, reached through the folder it is in.
    ///
    /// # Errors
    /// When a folder on the way can't be opened, or changed since the
    /// plan.
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

    /// `path`, a folder of the plan, reached through the folder it is in
    /// once it is found to be the folder planned.
    ///
    /// # Errors
    /// As [`Folders::source`].
    pub fn planned_folder(&mut self, path: &Path) -> Result<Source, Unreached> {
        self.open(path)?;
        self.source(path)
    }

    /// `folder`, opened, with only the folders on the way to it kept open.
    ///
    /// # Errors
    /// As [`Folders::source`].
    pub fn open(&mut self, folder: &Path) -> Result<BorrowedFd<'_>, Unreached> {
        self.open.retain(|open, _| folder.starts_with(open));
        self.reach(folder)?;
        self.open
            .get(folder)
            .map(AsFd::as_fd)
            .ok_or(Unreached::Failed(Errno::NOENT))
    }

    /// Opens `folder`, unless open already.
    fn reach(&mut self, folder: &Path) -> Result<(), Unreached> {
        if self.open.contains_key(folder) {
            return Ok(());
        }
        // Only found, never read: what is done in it needs its write and
        // search permission, not leave to list it.
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
            // Not walked by the plan: a folder the user's paths start in,
            // reached as the user named it.
            _ => open(folder, flags, Mode::empty())?,
        };
        self.open.insert(folder.to_path_buf(), fd);
        Ok(())
    }

    /// The folders open now.
    #[cfg(test)]
    pub fn opened(&self) -> impl Iterator<Item = &Path> {
        self.open.keys().map(PathBuf::as_path)
    }
}

#[cfg(test)]
mod tests;
