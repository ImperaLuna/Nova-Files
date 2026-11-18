use std::fs::{self, File, Metadata, OpenOptions};
use std::io;
use std::os::unix::fs::{FileExt as _, MetadataExt as _, OpenOptionsExt as _};
use std::path::Path;

use rustix::fs::{Mode, OFlags, SeekFrom, copy_file_range, ioctl_ficlone, openat, seek};
use rustix::io::Errno;

use nova_files_api::Failure;

use super::attrs::{copy_mode_and_times, copy_xattrs};
use super::control::Control;
use super::folders::At;
use super::stop::Stop;

const PART_MODE: u32 = 0o600;
pub const CHUNK: u64 = 0x80_0000;
const BUFFER: usize = 0x10_0000;

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

pub struct FileCopy<'a> {
    pub source: At<'a>,
    pub from: &'a Path,
    pub to: &'a Path,
    pub part: &'a Path,
}

pub fn copy_file(
    paths: &FileCopy<'_>,
    control: &Control,
    copied: &mut dyn FnMut(u64),
    place: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> Result<(), Stop> {
    let FileCopy {
        source,
        from,
        to,
        part,
    } = *paths;
    control.checkpoint()?;
    let source = open_source(source).map_err(|error| Stop::at(from, &error))?;
    if !source.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return Err(Stop::because(from, Failure::Unsupported));
    }
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
        place(part, to).map_err(|error| ends.writing(&error))
    });
    if done.is_err() {
        fs::remove_file(part).ok();
    }
    done
}

fn open_source(from: At<'_>) -> io::Result<File> {
    let flags = OFlags::RDONLY
        .union(OFlags::NOFOLLOW)
        .union(OFlags::NONBLOCK)
        .union(OFlags::CLOEXEC);
    Ok(File::from(openat(
        from.folder,
        from.name,
        flags,
        Mode::empty(),
    )?))
}

fn fill(ends: &Ends<'_>, control: &Control, copied: &mut dyn FnMut(u64)) -> Result<(), Stop> {
    let (source, target) = (ends.source, ends.target);
    let metadata = source.metadata().map_err(|error| ends.reading(&error))?;
    let size = metadata.len();
    if size == 0 {
        read_to_end(ends, control, copied)?;
    } else if ioctl_ficlone(target, source).is_ok() {
        copied(size);
    } else {
        for (start, end) in data_ranges(source, size) {
            copy_range(ends, (start, end), control, copied)?;
        }
        target.set_len(size).map_err(|error| ends.writing(&error))?;
    }
    if let Err(error) = copy_xattrs(source, target) {
        log::warn!(
            "can't read the attributes of {}: {error}",
            ends.from.display()
        );
    }
    let after = source.metadata().map_err(|error| ends.reading(&error))?;
    if version(&after) != version(&metadata) {
        return Err(Stop::because(ends.from, Failure::SourceChanged));
    }
    copy_mode_and_times(&metadata, target).map_err(|error| ends.writing(&error))?;
    target.sync_all().map_err(|error| ends.writing(&error))
}

pub fn version(metadata: &Metadata) -> (u64, i64, i64, i64, i64) {
    (
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.ctime(),
        metadata.ctime_nsec(),
    )
}

fn read_to_end(
    ends: &Ends<'_>,
    control: &Control,
    copied: &mut dyn FnMut(u64),
) -> Result<(), Stop> {
    let mut buffer = vec![0; BUFFER];
    let mut at = 0_u64;
    loop {
        control.checkpoint()?;
        let read = ends
            .source
            .read_at(&mut buffer, at)
            .map_err(|error| ends.reading(&error))?;
        if read == 0 {
            return Ok(());
        }
        ends.target
            .write_all_at(&buffer[..read], at)
            .map_err(|error| ends.writing(&error))?;
        copied(read as u64);
        at = at.saturating_add(read as u64);
    }
}

fn data_ranges(source: &File, size: u64) -> Vec<(u64, u64)> {
    let mut ranges = Vec::new();
    let mut at = 0;
    while at < size {
        let start = match seek(source, SeekFrom::Data(at)) {
            Ok(start) => start,
            Err(Errno::NXIO) => break,
            Err(_) => return vec![(0, size)],
        };
        let end = seek(source, SeekFrom::Hole(start)).map_or(size, |end| end.min(size));
        ranges.push((start, end));
        at = end;
    }
    ranges
}

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
            return read_and_write(ends, (at, end), control, copied);
        }
        copied(moved);
        at = read_at;
    }
}

// copy_file_range fails with these when it can't copy between the two files at all.
// https://man7.org/linux/man-pages/man2/copy_file_range.2.html
const fn kernel_cant_copy(errno: Errno) -> bool {
    matches!(
        errno,
        Errno::XDEV | Errno::INVAL | Errno::NOSYS | Errno::OPNOTSUPP
    )
}

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
