//! Crawling a folder into a [`Built`] index, depth first.
//!
//! Each folder is read whole (its entries and each entry's details, without
//! following links), sorted by name bytes and then walked one entry at a
//! time from an explicit stack, so a deep tree can't overflow the thread's
//! stack and no folder stays open while its subfolders are read. A folder
//! is added once it is read, after `policy` had its say on the folder and
//! on what it holds; it is closed once its last entry is added.
//!
//! The crawl stays on the crawled folder's drive: a folder that is another
//! drive's mount point is indexed but not entered. Entries that vanish
//! while the crawl runs are left out; a folder that can't be read is
//! indexed with no rows under it and [`Flags::UNREADABLE`]. `cancelled` is
//! asked before every read from the disk.

use std::ffi::OsString;
use std::fs::{self, Metadata};
use std::io;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{DirEntryExt as _, MetadataExt as _};
use std::path::{self, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::build::{Built, Facts, TooBig};
use crate::policy::{self, Zone};
use crate::{FileId, Flags, Kind, RowId, Stamp};

const NANOS_PER_SECOND: i64 = 1_000_000_000;

/// Why [`crawl`] gave no index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrawlError {
    /// The folder to crawl couldn't be read.
    Unreadable(io::ErrorKind),
    /// What was given to crawl isn't a folder.
    NotAFolder,
    /// `cancelled` said so.
    Cancelled,
    /// More entries or name bytes than an index holds.
    TooBig,
}

impl From<TooBig> for CrawlError {
    fn from(TooBig: TooBig) -> Self {
        Self::TooBig
    }
}

/// Crawls the folder at `root`, which the rules in `policy` treat as the
/// home folder, into an index.
///
/// # Errors
/// [`CrawlError`] when `root` can't be crawled or the crawl was cancelled.
pub fn crawl(root: &Path, cancelled: impl Fn() -> bool) -> Result<Built, CrawlError> {
    let crawled = now();
    let stop = |error: io::Error| CrawlError::Unreadable(error.kind());
    if cancelled() {
        return Err(CrawlError::Cancelled);
    }
    let root = path::absolute(root).map_err(stop)?;
    let details = fs::symlink_metadata(&root).map_err(stop)?;
    if !details.is_dir() {
        return Err(CrawlError::NotAFolder);
    }
    let children = read_children(&root, &cancelled).map_err(|reason| match reason {
        Stop::Cancelled => CrawlError::Cancelled,
        Stop::Failed(error) => stop(error),
    })?;
    let mut crawl = Crawl {
        built: Built::new(crawled),
        stack: Vec::new(),
        device: details.dev(),
        cancelled,
    };
    let row = crawl.built.push(
        None,
        root.as_os_str().as_bytes(),
        facts(&details, Kind::Folder),
    )?;
    crawl.stack.push(Frame {
        row,
        path: root,
        zone: Zone::Home,
        children: children.into_iter(),
    });
    crawl.run()?;
    Ok(crawl.built)
}

/// One entry of a folder, read along with the folder.
struct Child {
    name: OsString,
    inode: u64,
    details: io::Result<Metadata>,
}

/// A folder whose entries are being added.
struct Frame {
    row: RowId,
    path: PathBuf,
    zone: Zone,
    children: std::vec::IntoIter<Child>,
}

enum Stop {
    Cancelled,
    Failed(io::Error),
}

struct Crawl<C> {
    built: Built,
    stack: Vec<Frame>,
    /// The crawled folder's drive.
    device: u64,
    cancelled: C,
}

impl<C: Fn() -> bool> Crawl<C> {
    fn run(&mut self) -> Result<(), CrawlError> {
        while let Some(frame) = self.stack.last_mut() {
            let Some(child) = frame.children.next() else {
                self.built.close(frame.row);
                self.stack.pop();
                continue;
            };
            let (parent, zone, path) =
                (frame.row, frame.zone.clone(), frame.path.join(&child.name));
            self.add(parent, &zone, path, child)?;
        }
        Ok(())
    }

    /// Adds `child`, at `path` in folder `parent`, if `zone`'s rules keep it.
    fn add(
        &mut self,
        parent: RowId,
        zone: &Zone,
        path: PathBuf,
        child: Child,
    ) -> Result<(), CrawlError> {
        let name = child.name.as_bytes();
        let details = match child.details {
            Ok(details) => details,
            // It vanished after the folder was read.
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(_) if !policy::keeps_file(zone) => return Ok(()),
            Err(_) => {
                let facts = unread(FileId {
                    device: self.device,
                    inode: child.inode,
                });
                self.built.push(Some(parent), name, facts)?;
                return Ok(());
            }
        };
        if !details.is_dir() {
            if policy::keeps_file(zone) {
                let kind = if details.is_symlink() {
                    Kind::Link
                } else {
                    Kind::File
                };
                self.built.push(Some(parent), name, facts(&details, kind))?;
            }
            return Ok(());
        }
        let Some(inner) = policy::enters(zone, &child.name) else {
            return Ok(());
        };
        let folder = facts(&details, Kind::Folder);
        if details.dev() != self.device {
            let row = self.built.push(Some(parent), name, folder)?;
            self.built.flag(row, Flags::OTHER_DRIVE);
            return Ok(());
        }
        match read_children(&path, &self.cancelled) {
            Ok(children) if policy::marked(&names(&children)) => Ok(()),
            Ok(children) => {
                let row = self.built.push(Some(parent), name, folder)?;
                self.stack.push(Frame {
                    row,
                    path,
                    zone: inner,
                    children: children.into_iter(),
                });
                Ok(())
            }
            Err(Stop::Cancelled) => Err(CrawlError::Cancelled),
            Err(Stop::Failed(error)) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(Stop::Failed(_)) => {
                let row = self.built.push(Some(parent), name, folder)?;
                self.built.flag(row, Flags::UNREADABLE);
                Ok(())
            }
        }
    }
}

/// Reads the folder at `path`: every entry with its details, sorted by
/// name bytes.
fn read_children(path: &Path, cancelled: &impl Fn() -> bool) -> Result<Vec<Child>, Stop> {
    if cancelled() {
        return Err(Stop::Cancelled);
    }
    let mut children = Vec::new();
    for item in fs::read_dir(path).map_err(Stop::Failed)? {
        if cancelled() {
            return Err(Stop::Cancelled);
        }
        match item {
            Ok(item) => children.push(Child {
                name: item.file_name(),
                inode: item.ino(),
                details: item.metadata(),
            }),
            // An entry the folder lists but that can't be read is as good
            // as gone.
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(Stop::Failed(error)),
        }
    }
    children.sort_unstable_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));
    Ok(children)
}

fn names(children: &[Child]) -> Vec<OsString> {
    children.iter().map(|child| child.name.clone()).collect()
}

fn facts(details: &Metadata, kind: Kind) -> Facts {
    Facts {
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

/// An entry whose details couldn't be read: only its id is known.
const fn unread(id: FileId) -> Facts {
    Facts {
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
