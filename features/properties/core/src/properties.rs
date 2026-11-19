use std::ffi::OsString;
use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime};

use nova_files_api::{MODE_BITS, Unreadable};
use nova_files_messages::EventSink;
use nova_files_properties_api::{Account, Event, PathKind, Properties, PropertiesId};
use rustix::fs::{
    AtFlags, CWD, FileType, Mode, OFlags, StatVfsMountFlags, Statx, StatxFlags, StatxTimestamp,
    fstatvfs, openat, readlinkat, statx,
};
use rustix::process::geteuid;

use crate::accounts::{group_name, own_groups, user_name};
use nova_files_entries_core::list;

const THREAD_NAME: &str = "nova-files-properties";

// statx counts stx_blocks in 512-byte units.
// https://man7.org/linux/man-pages/man2/statx.2.html
const BLOCK: u64 = 512;

const ROOT: u32 = 0;

const AT_PATH: OFlags = OFlags::PATH.union(OFlags::NOFOLLOW).union(OFlags::CLOEXEC);

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

pub fn answer(events: &Arc<dyn EventSink>, id: PropertiesId, path: PathBuf) {
    let sink = Arc::clone(events);
    let asked = path.clone();
    let spawned = thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            sink.deliver(event(id, path)).ok();
        });
    if spawned.is_err() {
        let reason = Unreadable::Failed;
        let path = asked;
        events
            .deliver(Event::PropertiesUnreadable { id, path, reason })
            .ok();
    }
}

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

fn read(path: &Path) -> io::Result<Properties> {
    let at = openat(CWD, path, AT_PATH, Mode::empty())?;
    let stat = statx(&at, c"", AtFlags::EMPTY_PATH, WANTED)?;
    let kind = kind(&at, &stat)?;
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
        may_change: !is_link && may_change(geteuid().as_raw(), stat.stx_uid),
        groups: own_groups().into_iter().map(group).collect(),
        read_only,
    })
}

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

const fn may_change(user: u32, owner: u32) -> bool {
    user == ROOT || user == owner
}

fn time(mask: u32, field: StatxFlags, stamp: StatxTimestamp) -> Option<SystemTime> {
    if mask & field.bits() == 0 {
        return None;
    }
    since_epoch(stamp.tv_sec, stamp.tv_nsec)
}

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
