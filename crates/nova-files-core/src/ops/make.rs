//! Making one planned item at its target: a copy makes it anew (folders,
//! files through part files, links as links); a move renames it there,
//! and only across drives, where no rename reaches, copies it and then
//! removes the source, once the copy is whole and only if the source is
//! still what was copied.

use std::fs::{self, DirBuilder, File, Metadata};
use std::io;
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, symlink};
use std::path::Path;

use nova_files_api::Failure;
use rustix::fs::{Access, access};
use rustix::io::Errno;

use super::copy_file::{FileCopy, copy_file, version};
use super::journal::Record;
use super::place::{rename_no_replace, replace, replace_through};
use super::plan::{Item, Kind};
use super::running::Running;
use super::stop::Stop;

/// A folder being filled is the user's alone; it gets its source's mode
/// once full.
const FILLING_MODE: u32 = 0o700;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum How {
    #[default]
    Copy,
    /// Rename, and where the rename finds another drive, copy and then
    /// remove the source.
    Move,
}

impl How {
    pub const fn moves(self) -> bool {
        matches!(self, Self::Move)
    }
}

/// What making an item did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Made {
    /// How many of its bytes count as done without having been copied:
    /// holes in a sparse file, or a whole file moved by a rename.
    pub left: u64,
    /// A folder moved whole by one rename, all it holds with it.
    pub carried: bool,
}

/// One item to make, the `index`th of its job.
pub struct Making<'a> {
    pub item: &'a Item,
    pub target: &'a Path,
    pub index: usize,
    /// Whether it takes the place of the file at `target`.
    pub replacing: bool,
    pub how: How,
}

/// Makes the item at its target.
///
/// # Errors
/// [`Stop`] when cancelled, or at the path a step failed at.
pub fn make(running: &mut Running, making: &Making<'_>) -> Result<Made, Stop> {
    let Making { item, target, .. } = *making;
    let job = running.job.0;
    let hidden = |what: &str| target.with_file_name(format!(".nova-{what}-{job}-{}", making.index));
    let (part, backup) = (hidden("part"), hidden("backup"));
    if making.how == How::Move {
        if making.replacing {
            running.record(Record::Backup(&backup, target));
        }
        // Not an exchange: after one, the source's name would hold the
        // file it replaced until that is removed, and a crash between
        // would leave it there unrecorded.
        let renamed = if making.replacing {
            replace_through(&item.from, target, &backup)
        } else {
            rename_no_replace(&item.from, target)
        };
        match renamed {
            Ok(()) => {
                running.record(Record::Moved(&item.from, target));
                return Ok(Made {
                    left: size(item),
                    carried: item.kind == Kind::Folder,
                });
            }
            Err(error) if crosses_drives(&error) => {}
            Err(error) => return Err(failed_rename(item, target, &error)),
        }
    }
    let moving = making.how.moves() && item.kind != Kind::Folder;
    // What a move copies must be removable after, or it would stay twice.
    let before = if moving {
        Some(removable(&item.from)?)
    } else {
        None
    };
    let mut left = 0;
    match item.kind {
        Kind::Folder => DirBuilder::new()
            .mode(FILLING_MODE)
            .create(target)
            .map_err(|error| Stop::at(target, &error))?,
        Kind::File { size } => {
            let copied = copy_whole(running, making, &part, &backup)?;
            left = size.saturating_sub(copied);
        }
        Kind::Link => link(running, making, &part, &backup)?,
        Kind::Special => return Err(Stop::because(&item.from, Failure::Unsupported)),
    }
    if let Some(before) = before {
        // The copy is whole and has its name: only now may the source go.
        if let Err(stop) = remove_source(running, making, &before) {
            running.reporter.take_back(size(item).saturating_sub(left));
            return Err(stop);
        }
        running.record(Record::Moved(&item.from, target));
    } else {
        running.record(Record::Made(target));
    }
    Ok(Made {
        left,
        carried: false,
    })
}

/// Copies the file `making` names through the part file `part`, and
/// returns how many bytes it copied.
fn copy_whole(
    running: &mut Running,
    making: &Making<'_>,
    part: &Path,
    backup: &Path,
) -> Result<u64, Stop> {
    running.record(Record::Part(part));
    if making.replacing {
        running.record(Record::Backup(backup, making.target));
    }
    let mut copied = 0;
    let reporter = &mut running.reporter;
    let paths = FileCopy {
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
        |part, to| put(part, to, backup, making.replacing),
    );
    if let Err(stop) = copying {
        // The item is skipped or tried again from the start.
        running.reporter.take_back(copied);
        return Err(stop);
    }
    Ok(copied)
}

/// Makes the link `making` names, pointing where its source points, by
/// way of `part` when it replaces.
fn link(
    running: &mut Running,
    making: &Making<'_>,
    part: &Path,
    backup: &Path,
) -> Result<(), Stop> {
    let (from, target) = (&making.item.from, making.target);
    let points_to = fs::read_link(from).map_err(|error| Stop::at(from, &error))?;
    if making.replacing {
        running.record(Record::Part(part));
        running.record(Record::Backup(backup, target));
    }
    let made = if making.replacing { part } else { target };
    symlink(points_to, made).map_err(|error| Stop::at(target, &error))?;
    if making.replacing
        && let Err(error) = put(part, target, backup, making.replacing)
    {
        fs::remove_file(part).ok();
        return Err(Stop::at(target, &error));
    }
    Ok(())
}

/// Looks at `from`, a source a move is about to copy, and checks its
/// folder lets it be removed after.
fn removable(from: &Path) -> Result<Metadata, Stop> {
    let metadata = from
        .symlink_metadata()
        .map_err(|error| Stop::at(from, &error))?;
    if let Some(folder) = from.parent() {
        access(folder, Access::WRITE_OK).map_err(|errno| Stop::at(from, &errno.into()))?;
    }
    Ok(metadata)
}

/// Removes the source of the item `making` copied whole to its target,
/// unless it changed since `before` was looked at: the copy may then hold
/// an older version, and the source is the only one of the newer. The
/// copy's name is on the disk first, so a power cut between the two
/// drives can't lose both. When the source can't go, the copy goes
/// instead, so a retry starts afresh and nothing is left twice; unless it
/// replaced a file, which is gone already: then the copy stays.
fn remove_source(running: &Running, making: &Making<'_>, before: &Metadata) -> Result<(), Stop> {
    let Making { item, target, .. } = *making;
    let removed = running
        .shared
        .control
        .checkpoint()
        .map_err(Stop::from)
        .and_then(|()| {
            if let Some(folder) = target.parent() {
                File::open(folder)
                    .and_then(|folder| folder.sync_all())
                    .map_err(|error| Stop::at(target, &error))?;
            }
            let now = item
                .from
                .symlink_metadata()
                .map_err(|error| Stop::at(&item.from, &error))?;
            if !unchanged(before, &now) {
                return Err(Stop::because(&item.from, Failure::SourceChanged));
            }
            fs::remove_file(&item.from).map_err(|error| Stop::at(&item.from, &error))
        });
    if removed.is_err()
        && !making.replacing
        && let Err(error) = fs::remove_file(target)
    {
        log::warn!("{} is left twice: {error}", target.display());
    }
    removed
}

/// Whether `now` is still the entry `before` described.
fn unchanged(before: &Metadata, now: &Metadata) -> bool {
    (before.dev(), before.ino(), version(before)) == (now.dev(), now.ino(), version(now))
}

/// The bytes `item` counts for in the job's progress.
pub const fn size(item: &Item) -> u64 {
    match item.kind {
        Kind::File { size } => size,
        Kind::Folder | Kind::Link | Kind::Special => 0,
    }
}

/// Gives the whole file at `from` the name `to`, replacing what has it
/// only when `replacing`.
fn put(from: &Path, to: &Path, backup: &Path, replacing: bool) -> io::Result<()> {
    if replacing {
        replace(from, to, backup)
    } else {
        rename_no_replace(from, to)
    }
}

/// Whether `error` says a rename can't reach from one drive to another.
fn crosses_drives(error: &io::Error) -> bool {
    error.raw_os_error() == Some(Errno::XDEV.raw_os_error())
}

/// Where a move's rename of `item` to `target` failed with `error`: at
/// the source when it is gone, else at the target.
fn failed_rename(item: &Item, target: &Path, error: &io::Error) -> Stop {
    let gone = error.kind() == io::ErrorKind::NotFound && item.from.symlink_metadata().is_err();
    Stop::at(if gone { &item.from } else { target }, error)
}

#[cfg(test)]
mod tests;
