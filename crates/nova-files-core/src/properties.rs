//! Reading a path's properties for [`Command::Properties`]: one `statx`
//! that never follows a link, the link's target, whether its drive is
//! mounted read-only, and the names of its owner, its group and the user's
//! own groups.
//!
//! Each request reads on a thread of its own: a drive that stopped
//! answering, or a slow account lookup (LDAP), never holds up the core.
//!
//! [`Command::Properties`]: nova_files_api::Command::Properties

use std::ffi::OsString;
use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime};

use nova_files_api::{
    Account, Event, EventSink, MODE_BITS, PathKind, Properties, PropertiesId, Unreadable,
};
use rustix::fs::{
    AtFlags, CWD, FileType, Mode, OFlags, StatVfsMountFlags, Statx, StatxFlags, StatxTimestamp,
    fstatvfs, openat, readlinkat, statx,
};
use rustix::process::geteuid;

use crate::accounts::{group_name, own_groups, user_name};
use crate::list;

const THREAD_NAME: &str = "nova-files-properties";

/// `statx` counts blocks of this many bytes, whatever the drive's own.
const BLOCK: u64 = 512;

/// The superuser's number.
const ROOT: u32 = 0;

/// Opening a path only to ask about it: never its target, nothing read.
const AT_PATH: OFlags = OFlags::PATH.union(OFlags::NOFOLLOW).union(OFlags::CLOEXEC);

/// What a properties read asks `statx` for.
const WANTED: StatxFlags = StatxFlags::TYPE
    .union(StatxFlags::MODE)
    .union(StatxFlags::UID)
    .union(StatxFlags::GID)
    .union(StatxFlags::SIZE)
    .union(StatxFlags::BLOCKS)
    .union(StatxFlags::ATIME)
    .union(StatxFlags::MTIME)
    .union(StatxFlags::CTIME)
    .union(StatxFlags::BTIME);

/// Reads the properties of `path` on a thread of its own and hands the
/// answer to `events`.
pub fn answer(events: &Arc<dyn EventSink>, id: PropertiesId, path: PathBuf) {
    let sink = Arc::clone(events);
    let asked = path.clone();
    let spawned = thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            sink.deliver(event(id, path)).ok();
        });
    if spawned.is_err() {
        // Nothing could be read; the client can ask again.
        let reason = Unreadable::Failed;
        let path = asked;
        events
            .deliver(Event::PropertiesUnreadable { id, path, reason })
            .ok();
    }
}

/// The answer to request `id` for `path`.
fn event(id: PropertiesId, path: PathBuf) -> Event {
    match read(&path) {
        Ok(properties) => Event::Properties {
            id,
            properties: Arc::new(properties),
        },
        Err(error) => Event::PropertiesUnreadable {
            id,
            path,
            reason: list::unreadable(&error),
        },
    }
}

/// Reads the properties of `path`, through one handle on it so that every
/// answer is about the same thing, even if the path is swapped meanwhile.
fn read(path: &Path) -> io::Result<Properties> {
    let at = openat(CWD, path, AT_PATH, Mode::empty())?;
    let stat = statx(&at, c"", AtFlags::EMPTY_PATH, WANTED)?;
    let kind = kind(&at, &stat)?;
    // Asked of the handle, so a link answers for its own drive.
    let read_only = fstatvfs(&at)
        .ok()
        .map(|drive| drive.f_flag.contains(StatVfsMountFlags::RDONLY));
    let mask = stat.stx_mask;
    let is_link = matches!(kind, PathKind::Link { .. });
    Ok(Properties {
        path: path.to_owned(),
        kind,
        bytes: stat.stx_size,
        on_disk: stat.stx_blocks.saturating_mul(BLOCK),
        modified: time(mask, StatxFlags::MTIME, stat.stx_mtime),
        accessed: time(mask, StatxFlags::ATIME, stat.stx_atime),
        changed: time(mask, StatxFlags::CTIME, stat.stx_ctime),
        created: time(mask, StatxFlags::BTIME, stat.stx_btime),
        mode: u32::from(stat.stx_mode) & MODE_BITS,
        owner: user(stat.stx_uid),
        group: group(stat.stx_gid),
        // Linux keeps no permissions on a link.
        may_change: !is_link && may_change(geteuid().as_raw(), stat.stx_uid),
        groups: own_groups().into_iter().map(group).collect(),
        read_only,
    })
}

/// What `stat` says the thing `at` is, with a link's target.
fn kind(at: &OwnedFd, stat: &Statx) -> io::Result<PathKind> {
    Ok(match FileType::from_raw_mode(stat.stx_mode.into()) {
        FileType::Directory => PathKind::Folder,
        FileType::RegularFile => PathKind::File,
        FileType::Symlink => {
            let target = readlinkat(at, c"", Vec::new())?;
            PathKind::Link {
                target: PathBuf::from(OsString::from_vec(target.into_bytes())),
            }
        }
        _ => PathKind::Special,
    })
}

/// Whether `user` may change the permissions and group of what `owner`
/// owns.
const fn may_change(user: u32, owner: u32) -> bool {
    user == ROOT || user == owner
}

/// `stamp` as a time, if `mask` says the drive filled in `field` and the
/// system can hold it.
fn time(mask: u32, field: StatxFlags, stamp: StatxTimestamp) -> Option<SystemTime> {
    if mask & field.bits() == 0 {
        return None;
    }
    since_epoch(stamp.tv_sec, stamp.tv_nsec)
}

/// The time `seconds` and then `nanos` after the epoch (before it, for
/// negative seconds), if the system can hold it.
fn since_epoch(seconds: i64, nanos: u32) -> Option<SystemTime> {
    let nanos = Duration::from_nanos(u64::from(nanos));
    let at = u64::try_from(seconds).map_or_else(
        |_| SystemTime::UNIX_EPOCH.checked_sub(Duration::from_secs(seconds.unsigned_abs())),
        |after| SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(after)),
    );
    at?.checked_add(nanos)
}

fn user(id: u32) -> Account {
    Account {
        id,
        name: user_name(id),
    }
}

fn group(id: u32) -> Account {
    Account {
        id,
        name: group_name(id),
    }
}

#[cfg(test)]
mod tests;
