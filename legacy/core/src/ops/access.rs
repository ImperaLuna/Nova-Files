use std::collections::BTreeSet;
use std::ffi::{CString, OsStr};
use std::os::fd::{AsRawFd as _, OwnedFd};
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nova_files_api::{AccessChange, Failure, MODE_BITS, ModeChange, Outcome, Reach};
use rustix::fs::{
    AtFlags, CWD, Dir, FileType, Gid, Mode, OFlags, Statx, StatxFlags, Uid, chmodat, chownat,
    openat, statx,
};
use rustix::io::Errno;

use super::Ended;
use super::failure::failure;
use super::running::Running;
use super::stop::Stop;
use crate::mount_root::{Device, is_mount_root};

const AT_PATH: OFlags = OFlags::PATH.union(OFlags::NOFOLLOW).union(OFlags::CLOEXEC);

const LIST: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::CLOEXEC);

const WANTED: StatxFlags = StatxFlags::TYPE
    .union(StatxFlags::MODE)
    .union(StatxFlags::UID)
    .union(StatxFlags::GID);

const OWNER_LIST_AND_ENTER: u32 = 0o500;
const KEEP: u32 = u32::MAX;

struct Next {
    path: PathBuf,
    at: Place,
    inside: Option<Device>,
}

enum Task {
    Visit(Next),
    Close { path: PathBuf, at: Arc<OwnedFd> },
}

enum Place {
    Chosen,
    In { folder: Arc<OwnedFd>, name: CString },
}

pub fn change_access(running: &mut Running, paths: &[PathBuf], change: &AccessChange) -> Ended {
    if let Some(path) = paths.iter().find(|path| !path.is_absolute()) {
        return (
            Stop::because(path, Failure::InvalidName).outcome(),
            Vec::new(),
        );
    }
    let mut chosen: Vec<&PathBuf> = paths.iter().collect();
    chosen.sort_by_key(|path| path.components().count());
    let mut waiting: Vec<Task> = chosen
        .into_iter()
        .map(|path| {
            Task::Visit(Next {
                path: path.clone(),
                at: Place::Chosen,
                inside: None,
            })
        })
        .collect();
    let mut changed = BTreeSet::new();
    let mut result = Ok(());
    while let Some(task) = waiting.pop() {
        let done = match task {
            Task::Visit(next) => visit(running, change, next, &mut waiting),
            Task::Close { path, at } => {
                attempt(running, &path, || apply(&at, change, change.folders))
                    .map(|did| did.unwrap_or(false).then_some(path))
            }
        };
        match done {
            Ok(Some(path)) => {
                if let Some(folder) = path.parent() {
                    changed.insert(folder.to_path_buf());
                }
            }
            Ok(None) => {}
            Err(stop) => {
                result = Err(stop);
                break;
            }
        }
        running.reporter.item(0);
    }
    running.reporter.tell();
    let outcome = result.map_or_else(Stop::outcome, |()| Outcome::Done);
    (outcome, changed.into_iter().collect())
}

fn visit(
    running: &mut Running,
    change: &AccessChange,
    next: Next,
    waiting: &mut Vec<Task>,
) -> Result<Option<PathBuf>, Stop> {
    let Some((at, stat)) = attempt(running, &next.path, || reach(&next))? else {
        return Ok(None);
    };
    let file_type = FileType::from_raw_mode(stat.stx_mode.into());
    let own = (stat.stx_dev_major, stat.stx_dev_minor);
    let elsewhere = next.inside.is_some_and(|parent| {
        is_mount_root(stat.stx_attributes_mask, stat.stx_attributes, own, parent)
    });
    if file_type == FileType::Symlink || elsewhere {
        return Ok(None);
    }
    let folder = file_type == FileType::Directory;
    let modes = if folder { change.folders } else { change.files };
    let into = folder && change.reach == Reach::Inside;
    if into && modes.clear & OWNER_LIST_AND_ENTER != 0 {
        let Some(names) = attempt(running, &next.path, || names(&at))? else {
            return Ok(None);
        };
        let at = Arc::new(at);
        waiting.push(Task::Close {
            path: next.path.clone(),
            at: Arc::clone(&at),
        });
        queue(waiting, &next.path, &at, names, own);
        return Ok(None);
    }
    let did = attempt(running, &next.path, || apply(&at, change, modes))?.unwrap_or(false);
    if into {
        let names = attempt(running, &next.path, || names(&at))?.unwrap_or_default();
        queue(waiting, &next.path, &Arc::new(at), names, own);
    }
    Ok(did.then_some(next.path))
}

fn queue(
    waiting: &mut Vec<Task>,
    path: &Path,
    folder: &Arc<OwnedFd>,
    names: Vec<CString>,
    device: Device,
) {
    waiting.extend(names.into_iter().rev().map(|name| {
        Task::Visit(Next {
            path: path.join(OsStr::from_bytes(name.as_bytes())),
            at: Place::In {
                folder: Arc::clone(folder),
                name,
            },
            inside: Some(device),
        })
    }));
}

fn attempt<T>(
    running: &mut Running,
    path: &Path,
    mut step: impl FnMut() -> rustix::io::Result<T>,
) -> Result<Option<T>, Stop> {
    let done = running.retry_or_skip(path, || match step() {
        Ok(done) => Ok(Some(done)),
        Err(Errno::NOENT) => Ok(None),
        Err(errno) => Err(failure(&errno.into())),
    })?;
    Ok(done.flatten())
}

fn reach(next: &Next) -> rustix::io::Result<(OwnedFd, Statx)> {
    let at = match &next.at {
        Place::Chosen => openat(CWD, without_slash(&next.path), AT_PATH, Mode::empty())?,
        Place::In { folder, name } => openat(&**folder, name.as_c_str(), AT_PATH, Mode::empty())?,
    };
    let stat = look(&at)?;
    Ok((at, stat))
}

fn without_slash(path: &Path) -> PathBuf {
    path.components().collect()
}

fn apply(at: &OwnedFd, change: &AccessChange, modes: ModeChange) -> rustix::io::Result<bool> {
    let mut stat = look(at)?;
    let owner = change
        .owner
        .filter(|&owner| owner != stat.stx_uid && owner != KEEP);
    let group = change
        .group
        .filter(|&group| group != stat.stx_gid && group != KEEP);
    let new_owner = owner.is_some() || group.is_some();
    if new_owner {
        chownat(
            at,
            c"",
            owner.map(Uid::from_raw),
            group.map(Gid::from_raw),
            AtFlags::EMPTY_PATH,
        )?;
        stat = look(at)?;
    }
    let mode = u32::from(stat.stx_mode) & MODE_BITS;
    let wanted = modes.applied_to(mode);
    if wanted != mode {
        chmodat(
            CWD,
            by_proc(at),
            Mode::from_raw_mode(wanted),
            AtFlags::empty(),
        )
        .map_err(without_proc)?;
    }
    Ok(new_owner || wanted != mode)
}

fn look(at: &OwnedFd) -> rustix::io::Result<Statx> {
    statx(at, c"", AtFlags::EMPTY_PATH, WANTED)
}

// An O_PATH handle can't change modes; its /proc link reaches the same file.
// https://man7.org/linux/man-pages/man5/proc_pid_fd.5.html
fn by_proc(at: &OwnedFd) -> String {
    format!("/proc/self/fd/{}", at.as_raw_fd())
}

fn without_proc(errno: Errno) -> Errno {
    if errno == Errno::NOENT {
        Errno::OPNOTSUPP
    } else {
        errno
    }
}

fn names(at: &OwnedFd) -> rustix::io::Result<Vec<CString>> {
    let listed = openat(CWD, by_proc(at), LIST, Mode::empty()).map_err(without_proc)?;
    let mut names = Vec::new();
    for entry in Dir::new(listed)? {
        let name = entry?.file_name().to_owned();
        if !matches!(name.to_bytes(), b"." | b"..") {
            names.push(name);
        }
    }
    Ok(names)
}

#[cfg(test)]
mod tests;
