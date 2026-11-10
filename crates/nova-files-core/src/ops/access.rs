//! The job that changes owner, group and permissions.
//!
//! Each path is reached as the user named it, without following a link at
//! its end; everything inside a folder is reached from the folder's own
//! open handle, by name, never through a link and never by a long path,
//! so the change stays in the trees the user chose even if one changes
//! while it runs. A link is passed over: it keeps no permissions of its
//! own, and its target is not what was chosen. A folder where another
//! drive is mounted is passed over too, with all it holds.
//!
//! Each thing is changed before what it holds is looked at, so a folder
//! the user may not yet enter can be opened up and then gone into. The
//! owner and group change first, as that clears set-user and set-group;
//! the permission bits after.
//!
//! The job keeps no journal: changing again what was changed already
//! changes nothing, so after a crash the user may simply ask again.

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

/// Reaching a thing only to change it: never its target, nothing read.
const AT_PATH: OFlags = OFlags::PATH.union(OFlags::NOFOLLOW).union(OFlags::CLOEXEC);

/// Opening a reached folder again, to list it.
const LIST: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::CLOEXEC);

/// What the job asks of each thing.
const WANTED: StatxFlags = StatxFlags::TYPE
    .union(StatxFlags::MODE)
    .union(StatxFlags::UID)
    .union(StatxFlags::GID);

/// The owner's leave to list a folder and to enter it: a change that
/// takes either away changes a folder after what it holds.
const OWNER_LIST_AND_ENTER: u32 = 0o500;
/// The number the system reads as "keep the owner (or group) it has".
const KEEP: u32 = u32::MAX;

/// A thing waiting to be changed.
struct Next {
    path: PathBuf,
    at: Place,
    /// The drive of the folder it is in, when it was found inside one.
    inside: Option<Device>,
}

/// What the job does next.
enum Task {
    /// Reach a thing, change it, and, as the change reaches, queue what it
    /// holds.
    Visit(Next),
    /// Change a folder already reached, once what it holds is done: the
    /// change takes away the owner's leave to list or enter it.
    Close { path: PathBuf, at: Arc<OwnedFd> },
}

/// Where a thing waiting to be changed is.
enum Place {
    /// A chosen path, as the user named it.
    Chosen,
    /// `name` in the open folder `folder`, which stays open while any of
    /// what it holds waits.
    In { folder: Arc<OwnedFd>, name: CString },
}

/// Changes each of `paths` as `change` says.
pub fn change_access(running: &mut Running, paths: &[PathBuf], change: &AccessChange) -> Ended {
    if let Some(path) = paths.iter().find(|path| !path.is_absolute()) {
        return (
            Stop::because(path, Failure::InvalidName).outcome(),
            Vec::new(),
        );
    }
    // Deepest last, as the job takes from the end: one chosen inside
    // another is changed before the other can shut it away.
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

/// Changes `next` and, as `change` reaches, adds what it holds to
/// `waiting`. Returns its path when something changed.
///
/// # Errors
/// [`Stop`] when cancelled, or when a failure isn't the user's to answer.
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
        // Listed and gone into first, changed last.
        // Skipped (or gone): left as it is, with what it holds.
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

/// Queues `names`, in the open folder `folder` at `path` on drive
/// `device`, to be visited in the order listed.
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

/// Tries `step` on `path` until it works, asking the user what to do
/// when it may not. `None` when it is gone, or the user skips it.
///
/// # Errors
/// [`Stop`] when cancelled, or when a failure isn't the user's to answer.
fn attempt<T>(
    running: &mut Running,
    path: &Path,
    mut step: impl FnMut() -> rustix::io::Result<T>,
) -> Result<Option<T>, Stop> {
    let done = running.retry_or_skip(path, || match step() {
        Ok(done) => Ok(Some(done)),
        // Gone since it was listed: nothing left to change.
        Err(Errno::NOENT) => Ok(None),
        Err(errno) => Err(failure(&errno.into())),
    })?;
    Ok(done.flatten())
}

/// Opens a handle on `next` and looks at it.
fn reach(next: &Next) -> rustix::io::Result<(OwnedFd, Statx)> {
    let at = match &next.at {
        // Without a trailing slash, which would follow a link.
        Place::Chosen => openat(CWD, without_slash(&next.path), AT_PATH, Mode::empty())?,
        Place::In { folder, name } => openat(&**folder, name.as_c_str(), AT_PATH, Mode::empty())?,
    };
    let stat = look(&at)?;
    Ok((at, stat))
}

/// `path` with no `/` at its end, so naming it never follows a link.
fn without_slash(path: &Path) -> PathBuf {
    path.components().collect()
}

/// Gives the thing `at` `change`'s owner and group and `modes`'
/// permission bits. Whether anything changed.
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
        // The kernel may have taken set-user and set-group away, as it
        // should: what someone else wrote must not run as the new owner.
        // They stay off unless the change itself sets them.
        stat = look(at)?;
    }
    let mode = u32::from(stat.stx_mode) & MODE_BITS;
    let wanted = modes.applied_to(mode);
    if wanted != mode {
        // A handle that only reaches can't change permission bits itself;
        // the kernel's link to it can, and leads to that very thing.
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

/// Looks at the thing `at` again.
fn look(at: &OwnedFd) -> rustix::io::Result<Statx> {
    statx(at, c"", AtFlags::EMPTY_PATH, WANTED)
}

/// The kernel's link to the thing `at`, in `/proc`.
fn by_proc(at: &OwnedFd) -> String {
    format!("/proc/self/fd/{}", at.as_raw_fd())
}

/// `errno` from going through `/proc`. While its handle is open a thing
/// can't be gone, so a missing link means a system without `/proc`: the
/// job can't do its work there, which isn't the thing being gone.
fn without_proc(errno: Errno) -> Errno {
    if errno == Errno::NOENT {
        Errno::OPNOTSUPP
    } else {
        errno
    }
}

/// The names in the folder `at`, `.` and `..` left out.
fn names(at: &OwnedFd) -> rustix::io::Result<Vec<CString>> {
    // Opened again through the kernel's link to the handle: listing needs
    // leave to read the folder, not to look it up by name again.
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
