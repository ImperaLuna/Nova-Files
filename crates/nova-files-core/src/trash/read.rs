//! Reading the trash back: the trashes the user has, what each holds, and
//! where one entry came from.
//!
//! The trashes are the home trash and, at each local drive's mount point
//! (see `nova_files_index::local_mount_points`), `.Trash/<uid>` or
//! `.Trash-<uid>` where `folder` would pick it: each, with its `files/`
//! and `info/`, a real folder the user owns. A network mount is never
//! looked at, as a look can hang. A trash at a btrfs subvolume's top that
//! isn't a mount point isn't listed, though its entries are found by
//! their path.
//!
//! An entry is listed when its info file, a regular file opened without
//! following a link, reads: a `[Trash Info]` group with a `Path` key,
//! escaped as `info` writes it. A path never holds `..` or `.`; on a
//! drive's trash it is relative to the drive's top, or whole and inside
//! it; on the home trash it is whole. The `DeletionDate`, local time, is
//! kept when it reads, and is unknown otherwise; the entry is listed
//! either way. An info file whose entry isn't in `files/` names nothing
//! and is left out; so is an entry in `files/` without an info file, as
//! the spec says.

use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::io::Read as _;
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use chrono::{Local, NaiveDateTime, TimeZone as _};
use nova_files_api::{EntryKind, Trashed};
use rustix::fs::OFlags;
use rustix::process::getuid;

use super::folder::{OWN_PREFIX, SHARED, TrashFolder, is_own, mount_trash_at};
use super::info::{DATE_FORMAT, DATE_KEY, HEADER, INFO_SUFFIX, PATH_KEY};
use super::{FILES, Trashes};
use crate::percent::unescape;

/// The most an info file may hold to be read: a few paths' worth.
const MAX_INFO_BYTES: u64 = 64 * 1024;

/// The trashes there are now: the home trash at `home_trash`, when it is
/// there, and, when `drives`, each local drive's.
pub fn trashes(home_trash: Option<&Path>, drives: bool) -> Vec<TrashFolder> {
    let mut found: Vec<TrashFolder> = home_trash.and_then(home).into_iter().collect();
    if !drives {
        return found;
    }
    let uid = getuid().as_raw();
    let mut tops: Vec<_> = nova_files_index::local_mount_points().into_iter().collect();
    tops.sort();
    for top in tops {
        let roots = [
            top.join(SHARED).join(uid.to_string()),
            top.join(format!("{OWN_PREFIX}{uid}")),
        ];
        // The shared one first, as `folder` picks it when it is fit.
        if let Some(trash) = roots.iter().find_map(|root| mount_trash_at(root)) {
            add_new(&mut found, trash);
        }
    }
    found
}

/// Adds `trash` to `found` unless there, as one drive mounted twice is.
fn add_new(found: &mut Vec<TrashFolder>, trash: TrashFolder) {
    if !found.iter().any(|known| known.root == trash.root) {
        found.push(trash);
    }
}

/// The home trash at `home_trash`, when it is there and the user's own.
fn home(home_trash: &Path) -> Option<TrashFolder> {
    let root = home_trash.canonicalize().ok()?;
    is_own(&root).then_some(TrashFolder { root, topdir: None })
}

/// The trash `item` sits in, by its path alone: its `files/` folder's
/// trash, when that is the home trash or, when they count, a drive's.
/// No other mount is looked at.
pub fn trash_of(item: &Path, trashes: &Trashes) -> Option<TrashFolder> {
    let files = item.parent()?;
    let root = files.parent()?;
    if files.file_name()? != FILES {
        return None;
    }
    if let Some(home) = trashes.home.as_deref().and_then(home)
        && home.root == root
    {
        return Some(home);
    }
    trashes.drives.then(|| mount_trash_at(root)).flatten()
}

/// What `trash` holds.
pub fn list(trash: &TrashFolder) -> Vec<Trashed> {
    let Ok(infos) = fs::read_dir(trash.info()) else {
        return Vec::new();
    };
    infos
        .flatten()
        .filter_map(|info| {
            let stored = stored_of(&info.file_name())?;
            entry(trash, &stored)
        })
        .collect()
}

/// The entry `item` names, and its trash; `None` when it isn't one a
/// trash of `trashes` lists.
pub fn find(item: &Path, trashes: &Trashes) -> Option<(TrashFolder, Trashed)> {
    let trash = trash_of(item, trashes)?;
    let found = entry(&trash, item.file_name()?)?;
    Some((trash, found))
}

/// The entry stored as `stored` in `trash`, from its info file.
fn entry(trash: &TrashFolder, stored: &OsStr) -> Option<Trashed> {
    let item = trash.files().join(stored);
    let there = item.symlink_metadata().ok()?;
    let text = read_info(&info_of(trash, stored))?;
    let (field, deleted) = parse(&text)?;
    let original = original(trash, &field)?;
    let kind = if there.is_dir() {
        EntryKind::Folder
    } else {
        EntryKind::File
    };
    Some(Trashed {
        item,
        original,
        deleted,
        kind,
    })
}

/// The bytes of the info file at `info`: a regular file, opened without
/// following a link or waiting on a pipe, of at most [`MAX_INFO_BYTES`].
fn read_info(info: &Path) -> Option<Vec<u8>> {
    let flags = (OFlags::NOFOLLOW | OFlags::NONBLOCK).bits().cast_signed();
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(flags)
        .open(info)
        .ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_INFO_BYTES + 1).read_to_end(&mut bytes).ok()?;
    (u64::try_from(bytes.len()).ok()? <= MAX_INFO_BYTES).then_some(bytes)
}

/// Where the info file of the entry stored as `stored` in `trash` is.
pub fn info_of(trash: &TrashFolder, stored: &OsStr) -> PathBuf {
    let mut name = stored.to_os_string();
    name.push(INFO_SUFFIX);
    trash.info().join(name)
}

/// The stored name an info file named `name` is for.
fn stored_of(name: &OsStr) -> Option<OsString> {
    let stored = name.as_bytes().strip_suffix(INFO_SUFFIX.as_bytes())?;
    (!stored.is_empty()).then(|| OsString::from_vec(stored.to_vec()))
}

/// Where an entry whose info file says `field` was trashed from: a path
/// relative to the drive's top, or whole and inside it, for a drive's
/// trash; whole for the home trash. Never through `..` or `.`.
fn original(trash: &TrashFolder, field: &Path) -> Option<PathBuf> {
    let plain = field
        .components()
        .all(|part| matches!(part, Component::Normal(_) | Component::RootDir));
    if !plain {
        return None;
    }
    match (&trash.topdir, field.is_absolute()) {
        (None, true) => Some(field.to_path_buf()),
        (Some(top), true) => field.starts_with(top).then(|| field.to_path_buf()),
        (Some(top), false) => Some(top.join(field)),
        (None, false) => None,
    }
}

/// The path and deletion time an info file's text holds, or `None` when
/// it isn't an info file.
fn parse(text: &[u8]) -> Option<(PathBuf, Option<SystemTime>)> {
    let mut lines = text
        .split(|&byte| byte == b'\n')
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
        .filter(|line| !line.is_empty() && !line.starts_with(b"#"));
    if lines.next()? != HEADER.as_bytes() {
        return None;
    }
    let (mut path, mut deleted) = (None, None);
    for line in lines {
        // Another group: the info is over.
        if line.starts_with(b"[") {
            break;
        }
        if let Some(value) = line.strip_prefix(PATH_KEY.as_bytes()) {
            path = path.or_else(|| unescape(value));
        } else if let Some(value) = line.strip_prefix(DATE_KEY.as_bytes()) {
            deleted = deleted.or_else(|| date(value));
        }
    }
    let path = path.filter(|path| !path.as_os_str().is_empty())?;
    Some((path, deleted))
}

/// The time `value`, local `YYYY-MM-DDThh:mm:ss`, names, or `None` when
/// it doesn't read.
fn date(value: &[u8]) -> Option<SystemTime> {
    let text = std::str::from_utf8(value).ok()?;
    let naive = NaiveDateTime::parse_from_str(text, DATE_FORMAT).ok()?;
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(SystemTime::from)
}

#[cfg(test)]
mod tests;
