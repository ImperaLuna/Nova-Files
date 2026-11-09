//! Copying one file's bytes, safely.
//!
//! The copy goes to a part file (a hidden name the job picked) in the
//! target's folder, made with only the user's rights. It is filled by a
//! reflink when the filesystem shares blocks (btrfs, XFS), or else range
//! by range with `copy_file_range`, which lets the kernel or a network
//! filesystem copy without the bytes passing through here, or else with
//! plain reads and writes. Only ranges that hold data are copied, so a
//! sparse file stays sparse. Then it gets the source's attributes, mode
//! and times, is synced, and takes its real name without replacing
//! anything (see `place`). A failure or a cancel anywhere removes the part
//! file, so a real name never holds half a file.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::{FileExt as _, OpenOptionsExt as _};
use std::path::Path;

use rustix::fs::{Mode, OFlags, SeekFrom, copy_file_range, ioctl_ficlone, seek};
use rustix::io::Errno;

use nova_files_api::Failure;

use super::attrs::{copy_mode_and_times, copy_xattrs};
use super::control::Control;
use super::place::rename_no_replace;
use super::stop::Stop;

/// A part file is the user's alone until it is whole.
const PART_MODE: u32 = 0o600;
/// The most bytes copied between two checks for a pause or cancel: 8 MiB.
const CHUNK: u64 = 0x80_0000;
/// The buffer for plain reads and writes: 1 MiB.
const BUFFER: usize = 0x10_0000;

/// One file's copy: the open source and part file, and the paths a
/// failed step is named by.
struct Ends<'a> {
    source: &'a File,
    target: &'a File,
    from: &'a Path,
    to: &'a Path,
}

impl Ends<'_> {
    fn reading(&self, error: &io::Error) -> Stop {
        Stop::at(self.from, error)
    }

    fn writing(&self, error: &io::Error) -> Stop {
        Stop::at(self.to, error)
    }

    /// A step that both read and wrote failed: a full or read-only drive
    /// is the target's, anything else the source's.
    fn failed(&self, error: &io::Error) -> Stop {
        match error.kind() {
            io::ErrorKind::StorageFull
            | io::ErrorKind::QuotaExceeded
            | io::ErrorKind::ReadOnlyFilesystem
            | io::ErrorKind::FileTooLarge => self.writing(error),
            _ => self.reading(error),
        }
    }
}

/// Copies the file `from` to `to` through the part file `part`, calling
/// `copied` with each count of bytes copied.
///
/// # Errors
/// [`Stop`] when cancelled, or failed at `from` or `to`. The part file is
/// gone either way.
pub fn copy_file(
    from: &Path,
    to: &Path,
    part: &Path,
    control: &Control,
    copied: &mut dyn FnMut(u64),
) -> Result<(), Stop> {
    control.checkpoint()?;
    let source = open_source(from).map_err(|error| Stop::at(from, &error))?;
    let target = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(PART_MODE)
        .open(part)
        .map_err(|error| Stop::at(to, &error))?;
    let ends = Ends {
        source: &source,
        target: &target,
        from,
        to,
    };
    let done = fill(&ends, control, copied).and_then(|()| {
        control.checkpoint()?;
        rename_no_replace(part, to).map_err(|error| ends.writing(&error))
    });
    if done.is_err() {
        fs::remove_file(part).ok();
    }
    done
}

/// Opens `from` for reading without following a link it may have become.
fn open_source(from: &Path) -> io::Result<File> {
    let flags = OFlags::RDONLY
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);
    Ok(File::from(rustix::fs::open(from, flags, Mode::empty())?))
}

/// Fills `target` from `source`, gives it the source's attributes, mode
/// and times, and syncs it.
fn fill(ends: &Ends<'_>, control: &Control, copied: &mut dyn FnMut(u64)) -> Result<(), Stop> {
    let (source, target) = (ends.source, ends.target);
    let metadata = source.metadata().map_err(|error| ends.reading(&error))?;
    let size = metadata.len();
    if ioctl_ficlone(target, source).is_ok() {
        copied(size);
    } else {
        for (start, end) in data_ranges(source, size) {
            copy_range(ends, (start, end), control, copied)?;
        }
        // A hole at the end has no data to copy, only a length.
        target.set_len(size).map_err(|error| ends.writing(&error))?;
    }
    if let Err(error) = copy_xattrs(source, target) {
        log::warn!(
            "can't read the attributes of {}: {error}",
            ends.from.display()
        );
    }
    // A file written to while it was copied may be torn; it isn't kept.
    let after = source.metadata().map_err(|error| ends.reading(&error))?;
    if after.len() != size || after.modified().ok() != metadata.modified().ok() {
        return Err(Stop::because(ends.from, Failure::SourceChanged));
    }
    copy_mode_and_times(&metadata, target).map_err(|error| ends.writing(&error))?;
    target.sync_all().map_err(|error| ends.writing(&error))
}

/// The ranges of `source` that hold data, in order. A filesystem that
/// can't tell holes from data has one range, the whole file.
fn data_ranges(source: &File, size: u64) -> Vec<(u64, u64)> {
    let mut ranges = Vec::new();
    let mut at = 0;
    while at < size {
        let start = match seek(source, SeekFrom::Data(at)) {
            Ok(start) => start,
            // No data past `at`: the rest is a hole.
            Err(Errno::NXIO) => break,
            Err(_) => return vec![(0, size)],
        };
        let end = seek(source, SeekFrom::Hole(start)).map_or(size, |end| end.min(size));
        ranges.push((start, end));
        at = end;
    }
    ranges
}

/// Copies bytes `start..end` of `source` to the same place in `target`.
fn copy_range(
    ends: &Ends<'_>,
    (start, end): (u64, u64),
    control: &Control,
    copied: &mut dyn FnMut(u64),
) -> Result<(), Stop> {
    let (source, target) = (ends.source, ends.target);
    let mut at = start;
    loop {
        let left = end.saturating_sub(at);
        if left == 0 {
            return Ok(());
        }
        control.checkpoint()?;
        let wanted = usize::try_from(left.min(CHUNK)).unwrap_or(BUFFER);
        let (mut read_at, mut write_at) = (at, at);
        let moved = match copy_file_range(
            source,
            Some(&mut read_at),
            target,
            Some(&mut write_at),
            wanted,
        ) {
            Ok(moved) => moved as u64,
            Err(errno) if kernel_cant_copy(errno) => {
                return read_and_write(ends, (at, end), control, copied);
            }
            Err(errno) => return Err(ends.failed(&errno.into())),
        };
        if moved == 0 {
            // Some filesystems copy nothing before the end of a file;
            // plain reads tell a file that shrank from one that didn't.
            return read_and_write(ends, (at, end), control, copied);
        }
        copied(moved);
        // The kernel moved the offset past what it copied.
        at = read_at;
    }
}

/// Whether `errno` says `copy_file_range` can't copy between these two
/// files at all, so plain reads and writes must.
const fn kernel_cant_copy(errno: Errno) -> bool {
    matches!(
        errno,
        Errno::XDEV | Errno::INVAL | Errno::NOSYS | Errno::OPNOTSUPP
    )
}

/// Copies bytes `start..end` with plain reads and writes.
fn read_and_write(
    ends: &Ends<'_>,
    (start, end): (u64, u64),
    control: &Control,
    copied: &mut dyn FnMut(u64),
) -> Result<(), Stop> {
    let (source, target) = (ends.source, ends.target);
    let mut buffer = vec![0; BUFFER];
    let mut at = start;
    while at < end {
        control.checkpoint()?;
        let wanted = usize::try_from(end - at).map_or(BUFFER, |left| left.min(BUFFER));
        let read = source
            .read_at(&mut buffer[..wanted], at)
            .map_err(|error| ends.reading(&error))?;
        if read == 0 {
            // It shrank since it was planned.
            return Err(Stop::because(ends.from, Failure::SourceChanged));
        }
        target
            .write_all_at(&buffer[..read], at)
            .map_err(|error| ends.writing(&error))?;
        copied(read as u64);
        at = at.saturating_add(read as u64);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
