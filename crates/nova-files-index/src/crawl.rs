use std::ffi::OsString;
use std::fs::{self, File, Metadata};
use std::io;
use std::io::Read as _;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{DirEntryExt as _, MetadataExt as _};
use std::path::{self, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rustix::fs::{Mode, OFlags};

use crate::build::{Built, TooBig};
use crate::mounts::{MountCheck, SystemMounts};
use crate::policy::{self, Zone};
use crate::{Details, FileId, Flags, Kind, RowId, Stamp};

const NANOS_PER_SECOND: i64 = 1_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrawlError {
    Unreadable(io::ErrorKind),
    NotAFolder,
    Cancelled,
    TooBig,
}

impl From<TooBig> for CrawlError {
    fn from(TooBig: TooBig) -> Self {
        Self::TooBig
    }
}

pub fn crawl(root: &Path, cancelled: impl Fn() -> bool) -> Result<Built, CrawlError> {
    crawl_with(root, SystemMounts::default(), cancelled)
}

pub fn crawl_with(
    root: &Path,
    mounts: impl MountCheck,
    cancelled: impl Fn() -> bool,
) -> Result<Built, CrawlError> {
    let crawled = now();
    let unreadable = |error: io::Error| CrawlError::Unreadable(error.kind());
    if cancelled() {
        return Err(CrawlError::Cancelled);
    }
    let root = path::absolute(root).map_err(unreadable)?;
    let details = fs::symlink_metadata(&root).map_err(unreadable)?;
    if !details.is_dir() {
        return Err(CrawlError::NotAFolder);
    }
    let children = read_children(&root, &cancelled).map_err(|reason| match reason {
        Stop::Cancelled => CrawlError::Cancelled,
        Stop::Gone => CrawlError::Unreadable(io::ErrorKind::NotFound),
        Stop::Failed(error) => unreadable(error),
    })?;
    let mut crawl = Crawl {
        built: Built::new(crawled),
        stack: Vec::new(),
        mounts,
        cancelled,
    };
    let row = crawl.built.push(
        None,
        root.as_os_str().as_bytes(),
        facts(&details, Kind::Folder),
    )?;
    crawl.stack.push(Frame {
        row,
        device: details.dev(),
        path: root,
        zone: Zone::Home,
        children: children.into_iter(),
    });
    crawl.run()?;
    Ok(crawl.built)
}

pub struct Child {
    pub name: OsString,
    pub inode: u64,
    pub details: io::Result<Metadata>,
}

struct Frame {
    row: RowId,
    device: u64,
    path: PathBuf,
    zone: Zone,
    children: std::vec::IntoIter<Child>,
}

pub enum Stop {
    Cancelled,
    Gone,
    Failed(io::Error),
}

fn stop(error: io::Error) -> Stop {
    if error.kind() == io::ErrorKind::NotFound {
        Stop::Gone
    } else {
        Stop::Failed(error)
    }
}

struct Crawl<M, C> {
    built: Built,
    stack: Vec<Frame>,
    mounts: M,
    cancelled: C,
}

struct Spot {
    parent: RowId,
    zone: Zone,
    device: u64,
    path: PathBuf,
}

impl<M: MountCheck, C: Fn() -> bool> Crawl<M, C> {
    fn run(&mut self) -> Result<(), CrawlError> {
        while let Some(frame) = self.stack.last_mut() {
            let Some(child) = frame.children.next() else {
                self.built.close(frame.row);
                self.stack.pop();
                continue;
            };
            let spot = Spot {
                parent: frame.row,
                zone: frame.zone.clone(),
                device: frame.device,
                path: frame.path.join(&child.name),
            };
            self.add(spot, child)?;
        }
        Ok(())
    }

    fn add(&mut self, spot: Spot, child: Child) -> Result<(), CrawlError> {
        let Spot {
            parent,
            zone,
            device,
            path,
        } = spot;
        let name = child.name.as_bytes();
        let details = match child.details.map_err(stop) {
            Ok(details) => details,
            Err(Stop::Gone | Stop::Cancelled) => return Ok(()),
            Err(Stop::Failed(_)) => {
                if policy::keeps_file(&zone) {
                    let id = FileId {
                        device,
                        inode: child.inode,
                    };
                    self.built.push(Some(parent), name, unread(id))?;
                }
                return Ok(());
            }
        };
        if !details.is_dir() {
            if policy::keeps_file(&zone) {
                let kind = if details.is_symlink() {
                    Kind::Link
                } else {
                    Kind::File
                };
                self.built.push(Some(parent), name, facts(&details, kind))?;
            }
            return Ok(());
        }
        let Some(inner) = policy::enters(&zone, &child.name) else {
            return Ok(());
        };
        let folder = facts(&details, Kind::Folder);
        if self.mounts.is_mount_root(&path) {
            let row = self.built.push(Some(parent), name, folder)?;
            self.built.flag(row, Flags::OTHER_DRIVE);
            return Ok(());
        }
        match self.read_folder(&path) {
            Ok(None) | Err(Stop::Gone) => Ok(()),
            Ok(Some(children)) => {
                let row = self.built.push(Some(parent), name, folder)?;
                self.stack.push(Frame {
                    row,
                    device: details.dev(),
                    path,
                    zone: inner,
                    children: children.into_iter(),
                });
                Ok(())
            }
            Err(Stop::Cancelled) => Err(CrawlError::Cancelled),
            Err(Stop::Failed(_)) => {
                let row = self.built.push(Some(parent), name, folder)?;
                self.built.flag(row, Flags::UNREADABLE);
                Ok(())
            }
        }
    }

    fn read_folder(&self, path: &Path) -> Result<Option<Vec<Child>>, Stop> {
        let children = read_children(path, &self.cancelled)?;
        let names: Vec<OsString> = children.iter().map(|child| child.name.clone()).collect();
        if policy::marked(&names) {
            return Ok(None);
        }
        if names.iter().any(|name| name == policy::CACHE_TAG) {
            if (self.cancelled)() {
                return Err(Stop::Cancelled);
            }
            if has_cache_signature(&path.join(policy::CACHE_TAG)) {
                return Ok(None);
            }
        }
        Ok(Some(children))
    }
}

pub fn has_cache_signature(path: &Path) -> bool {
    let mut start = [0; policy::CACHE_TAG_SIGNATURE.len()];
    let flags = OFlags::RDONLY
        .union(OFlags::NONBLOCK)
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);
    rustix::fs::open(path, flags, Mode::empty())
        .map(File::from)
        .map_err(io::Error::from)
        .and_then(|mut file| {
            if !file.metadata()?.is_file() {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            file.read_exact(&mut start)
        })
        .is_ok_and(|()| policy::is_cache_signature(&start))
}

pub fn read_children(path: &Path, cancelled: &impl Fn() -> bool) -> Result<Vec<Child>, Stop> {
    if cancelled() {
        return Err(Stop::Cancelled);
    }
    let mut children = Vec::new();
    for item in fs::read_dir(path).map_err(stop)? {
        if cancelled() {
            return Err(Stop::Cancelled);
        }
        match item.map_err(stop) {
            Ok(item) => children.push(Child {
                name: item.file_name(),
                inode: item.ino(),
                details: item.metadata(),
            }),
            Err(Stop::Gone | Stop::Cancelled) => {}
            Err(failed) => return Err(failed),
        }
    }
    children.sort_unstable_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));
    Ok(children)
}

pub fn facts(details: &Metadata, kind: Kind) -> Details {
    Details {
        kind,
        flags: Flags::default(),
        id: FileId {
            device: details.dev(),
            inode: details.ino(),
        },
        size: details.size(),
        modified: nanos(details.mtime(), details.mtime_nsec()),
        created: details.created().ok().and_then(stamp),
    }
}

const fn unread(id: FileId) -> Details {
    Details {
        kind: Kind::File,
        flags: Flags::UNREADABLE,
        id,
        size: 0,
        modified: None,
        created: None,
    }
}

fn nanos(seconds: i64, nanos: i64) -> Option<Stamp> {
    seconds
        .checked_mul(NANOS_PER_SECOND)?
        .checked_add(nanos)
        .map(Stamp)
}

fn stamp(time: SystemTime) -> Option<Stamp> {
    let since = time.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(since.as_nanos()).ok().map(Stamp)
}

fn now() -> Option<Stamp> {
    stamp(SystemTime::now())
}

#[cfg(test)]
mod tests;
