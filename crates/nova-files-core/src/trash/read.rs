//! Reading the trash back: the trashes the user has, what each holds, and
//! where one entry came from.
//!
//! The trashes are the home trash and, at each mount point the system
//! lists, `.Trash/<uid>` and `.Trash-<uid>` where they exist as the
//! user's own folders. A trash at a btrfs subvolume's top that isn't a
//! mount point is not found this way.
//!
//! An entry is listed when its info file reads: a `[Trash Info]` group
//! with a `Path` key, escaped as `info` writes it. A path relative to the
//! mount's top must not leave it (`..`); a home trash's path must be
//! whole. The `DeletionDate`, local time, is kept when it reads, and is
//! unknown otherwise; the entry is listed either way. An info file whose
//! entry isn't in `files/` names nothing and is left out; so is an entry
//! in `files/` without an info file, as the spec says.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::os::unix::fs::MetadataExt as _;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use chrono::{Local, NaiveDateTime, TimeZone as _};
use nova_files_api::{EntryKind, Trashed};
use rustix::process::getuid;

use super::folder::{OWN_PREFIX, SHARED, TrashFolder};
use super::info::{DATE_FORMAT, DATE_KEY, HEADER, INFO_SUFFIX, PATH_KEY};
use crate::percent::unescape;
use crate::private_file::{self, Read};

/// The most an info file may hold to be read: a few paths' worth.
const MAX_INFO_BYTES: usize = 64 * 1024;

/// The trashes the user has: the home trash at `home_trash`, when it is
/// there, and, when `drives`, each mount's.
pub fn trashes(home_trash: Option<&Path>, drives: bool) -> Vec<TrashFolder> {
    let uid = getuid().as_raw();
    let mut found: Vec<TrashFolder> = home_trash
        .and_then(|home| home.canonicalize().ok())
        .map(|root| TrashFolder { root, topdir: None })
        .into_iter()
        .collect();
    if !drives {
        return found;
    }
    let mut tops: Vec<_> = nova_files_index::system_mount_points()
        .into_iter()
        .collect();
    tops.sort();
    for top in tops {
        for root in [
            top.join(SHARED).join(uid.to_string()),
            top.join(format!("{OWN_PREFIX}{uid}")),
        ] {
            let mine = root
                .symlink_metadata()
                .is_ok_and(|there| there.is_dir() && there.uid() == uid);
            if mine && !found.iter().any(|known| known.root == root) {
                found.push(TrashFolder {
                    root,
                    topdir: Some(top.clone()),
                });
            }
        }
    }
    found
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

/// The entry `item` names, where it sits in one of the trashes
/// `trashes` lists, and that trash; `None` when it isn't one.
pub fn find(item: &Path, trashes: &[TrashFolder]) -> Option<(TrashFolder, Trashed)> {
    let files = item.parent()?;
    let stored = item.file_name()?;
    let trash = trashes.iter().find(|trash| trash.files() == files)?;
    let found = entry(trash, stored)?;
    Some((trash.clone(), found))
}

/// The entry stored as `stored` in `trash`, from its info file.
fn entry(trash: &TrashFolder, stored: &OsStr) -> Option<Trashed> {
    let item = trash.files().join(stored);
    let there = item.symlink_metadata().ok()?;
    let info = info_of(trash, stored);
    let Read::Bytes(text) = private_file::read(&info, MAX_INFO_BYTES) else {
        return None;
    };
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
/// relative to the mount's top for a mount's trash, whole otherwise.
fn original(trash: &TrashFolder, field: &Path) -> Option<PathBuf> {
    if field.is_absolute() {
        return Some(field.to_path_buf());
    }
    let leaves = field
        .components()
        .any(|part| !matches!(part, Component::Normal(_)));
    if leaves {
        return None;
    }
    trash.topdir.as_ref().map(|top| top.join(field))
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
