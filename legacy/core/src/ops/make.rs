use std::ffi::OsString;
use std::fs::{self, DirBuilder, Metadata};
use std::io;
use std::os::unix::ffi::OsStringExt as _;
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, symlink};
use std::path::{Path, PathBuf};

use nova_files_api::Failure;
use rustix::fs::{Access, AtFlags, accessat, readlinkat, unlinkat};
use rustix::io::Errno;

use super::copy_file::{CHUNK, FileCopy, copy_file, version};
use super::folders::{At, Unreached};
use super::journal::{BACKUP_PREFIX, PART_PREFIX, Record};
use super::keep_both::take_free_name;
use super::place::{rename_no_replace, rename_no_replace_at, replace, replace_through, same_entry};
use super::plan::{Item, Kind};
use super::running::Running;
use super::stop::Stop;
use super::sync::sync_folder;
use nova_files_jobs::set_aside::Aside;

const FILLING_MODE: u32 = 0o700;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum How {
    #[default]
    Copy,
    Move,
}

impl How {
    pub const fn moves(self) -> bool {
        matches!(self, Self::Move)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Made {
    pub left: u64,
    pub carried: bool,
}

pub struct Making<'a> {
    pub item: &'a Item,
    pub target: &'a Path,
    pub index: usize,
    pub replacing: bool,
    pub how: How,
}

pub fn make(running: &mut Running, making: &Making<'_>) -> Result<Made, Stop> {
    let Making { item, target, .. } = *making;
    let source = running
        .folders
        .source(&item.from)
        .map_err(|why| unreached(&item.from, why))?;
    let from = source.at();
    let names = running.names;
    let hidden = |prefix: &str| target.with_file_name(format!("{prefix}{names}-{}", making.index));
    let (part, backup) = (hidden(PART_PREFIX), hidden(BACKUP_PREFIX));
    if making.how == How::Move {
        if making.replacing {
            running.record_backup(&backup, target)?;
        }
        let renamed = if making.replacing {
            replace_through(from, target, &backup).map(Some)
        } else {
            rename_no_replace_at(from, target).map(|()| None)
        };
        match renamed {
            Ok(left) => {
                set_aside(running, making, left.as_deref());
                running.record(Record::Moved(&item.from, target));
                return Ok(Made {
                    left: item.size(),
                    carried: item.kind == Kind::Folder,
                });
            }
            Err(error) if crosses_drives(&error) => {}
            Err(error) => return Err(failed_rename(item, from, target, &error)),
        }
    }
    let moving = making.how.moves() && item.kind != Kind::Folder;
    let before = if moving {
        Some(removable(from, &item.from)?)
    } else {
        None
    };
    let mut left = 0;
    match item.kind {
        Kind::Folder => {
            DirBuilder::new()
                .mode(FILLING_MODE)
                .create(target)
                .map_err(|error| Stop::at(target, &error))?;
            if making.how.moves() {
                sync_folder_of(target)?;
            }
        }
        Kind::File { size } => {
            let copied = copy_whole(running, making, from, &part, &backup)?;
            left = size.saturating_sub(copied);
        }
        Kind::Link => link(running, making, from, &part, &backup)?,
        Kind::Special => return Err(Stop::because(&item.from, Failure::Unsupported)),
    }
    if let Some(before) = before {
        let copy = target.symlink_metadata().ok();
        if let Err(stop) = remove_source(running, making, from, &before, copy.as_ref()) {
            running.reporter.take_back(item.size().saturating_sub(left));
            return Err(stop);
        }
        running.record(Record::Moved(&item.from, target));
    } else {
        running.record(Record::Made(target, &item.from));
    }
    Ok(Made {
        left,
        carried: false,
    })
}

fn unreached(path: &Path, unreached: Unreached) -> Stop {
    match unreached {
        Unreached::Changed => Stop::because(path, Failure::SourceChanged),
        Unreached::Failed(errno) => Stop::at(path, &errno.into()),
    }
}

fn copy_whole(
    running: &mut Running,
    making: &Making<'_>,
    from: At<'_>,
    part: &Path,
    backup: &Path,
) -> Result<u64, Stop> {
    running.record(Record::Part(part));
    if making.replacing {
        running.record_backup(backup, making.target)?;
    }
    if making.item.size() > CHUNK {
        running.settle();
    }
    let mut copied = 0;
    let mut left = None;
    let reporter = &mut running.reporter;
    let paths = FileCopy {
        source: from,
        from: &making.item.from,
        to: making.target,
        part,
    };
    let copying = copy_file(
        &paths,
        &running.shared.control,
        &mut |bytes| {
            copied += bytes;
            reporter.bytes(bytes);
        },
        |part, to| {
            left = put(part, to, backup, making.replacing)?;
            Ok(())
        },
    );
    if let Err(stop) = copying {
        running.reporter.take_back(copied);
        return Err(stop);
    }
    set_aside(running, making, left.as_deref());
    Ok(copied)
}

fn link(
    running: &mut Running,
    making: &Making<'_>,
    from: At<'_>,
    part: &Path,
    backup: &Path,
) -> Result<(), Stop> {
    let target = making.target;
    let points_to = readlinkat(from.folder, from.name, Vec::new())
        .map(|points_to| PathBuf::from(OsString::from_vec(points_to.into_bytes())))
        .map_err(|errno| Stop::at(&making.item.from, &errno.into()))?;
    if making.replacing {
        running.record(Record::Part(part));
        running.record_backup(backup, target)?;
    }
    let made = if making.replacing { part } else { target };
    symlink(points_to, made).map_err(|error| Stop::at(target, &error))?;
    if making.replacing {
        match put(part, target, backup, making.replacing) {
            Ok(left) => set_aside(running, making, left.as_deref()),
            Err(error) => {
                fs::remove_file(part).ok();
                return Err(Stop::at(target, &error));
            }
        }
    }
    Ok(())
}

fn removable(from: At<'_>, path: &Path) -> Result<Metadata, Stop> {
    let metadata = from.look().map_err(|error| Stop::at(path, &error))?;
    accessat(
        from.folder,
        from.folder_path(),
        Access::WRITE_OK,
        AtFlags::empty(),
    )
    .map_err(|errno| Stop::at(path, &errno.into()))?;
    Ok(metadata)
}

fn remove_source(
    running: &Running,
    making: &Making<'_>,
    from: At<'_>,
    before: &Metadata,
    copy: Option<&Metadata>,
) -> Result<(), Stop> {
    let Making { item, target, .. } = *making;
    let removed = running
        .shared
        .control
        .checkpoint()
        .map_err(Stop::from)
        .and_then(|()| {
            sync_folder_of(target)?;
            let now = from.look().map_err(|error| Stop::at(&item.from, &error))?;
            if !unchanged(before, &now) {
                return Err(Stop::because(&item.from, Failure::SourceChanged));
            }
            unlinkat(from.folder, from.name, AtFlags::empty())
                .map_err(|errno| Stop::at(&item.from, &errno.into()))
        });
    let ours = target
        .symlink_metadata()
        .is_ok_and(|now| copy.is_some_and(|copy| same_entry(copy, &now)));
    if removed.is_err()
        && !making.replacing
        && ours
        && let Err(error) = fs::remove_file(target)
    {
        log::warn!("{} is left twice: {error}", target.display());
    }
    removed
}

fn sync_folder_of(target: &Path) -> Result<(), Stop> {
    target.parent().map_or(Ok(()), |folder| {
        sync_folder(folder).map_err(|error| Stop::at(target, &error))
    })
}

fn unchanged(before: &Metadata, now: &Metadata) -> bool {
    (before.dev(), before.ino(), version(before)) == (now.dev(), now.ino(), version(now))
}

fn put(from: &Path, to: &Path, backup: &Path, replacing: bool) -> io::Result<Option<PathBuf>> {
    if replacing {
        replace(from, to, backup).map(Some)
    } else {
        rename_no_replace(from, to).map(|()| None)
    }
}

fn set_aside(running: &mut Running, making: &Making<'_>, left: Option<&Path>) {
    let Some(left) = left else {
        return;
    };
    let target = making.target;
    let ours = running.placed_here(target);
    match running.set_aside((making.index, left), target) {
        Aside::Kept(item) => {
            if !ours {
                running.replaced.push((target.to_path_buf(), item));
            }
        }
        Aside::Gone => {}
        Aside::Nowhere => {
            if let Err(error) = fs::remove_file(left) {
                log::warn!("can't remove the replaced {}: {error}", left.display());
            }
        }
        Aside::Refused => match take_free_name(left, target) {
            Ok(kept) => log::warn!(
                "the replaced {} is kept as {}",
                target.display(),
                kept.display()
            ),
            Err(error) => log::warn!(
                "the replaced {} stays at {}: {error}",
                target.display(),
                left.display()
            ),
        },
    }
}

fn crosses_drives(error: &io::Error) -> bool {
    error.raw_os_error() == Some(Errno::XDEV.raw_os_error())
}

fn failed_rename(item: &Item, from: At<'_>, target: &Path, error: &io::Error) -> Stop {
    let gone = error.kind() == io::ErrorKind::NotFound && from.look().is_err();
    Stop::at(if gone { &item.from } else { target }, error)
}

#[cfg(test)]
mod tests;
