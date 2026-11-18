use std::fs::File;
use std::io::{self, BufReader, Read, Write};

use zip::ZipArchive;
use zip::extra_fields::ExtraField;

use crate::decompress::tar_stream;
use crate::ending::Ending;
use crate::format::{self, Format};
use crate::seven::Seven;
use crate::wire::{self, Clock, Entry, Kind};

pub const MAX_LINK: u64 = 4_096;
pub const FILE_MODE: u32 = 0o644;
pub const FOLDER_MODE: u32 = 0o755;
const TYPE_BITS: u32 = 0o170_000;
const LINK_TYPE: u32 = 0o120_000;

pub fn list(mut archive: File, seven: Seven<'_>, out: &mut impl Write) -> Ending {
    let result = match format::sniff(&mut archive) {
        Ok(Some(Format::Seven)) => return seven.list(out),
        Ok(Some(Format::Zip)) => list_zip(archive, out),
        Ok(Some(Format::Tar(compression))) => {
            tar_stream(archive, compression).and_then(|stream| list_tar(stream, out))
        }
        Ok(None) => return Ending::Unsupported,
        Err(_) => return Ending::Broken,
    };
    match result {
        Ok(()) => Ending::Done,
        Err(_) => Ending::Broken,
    }
}

fn list_zip(archive: File, out: &mut impl Write) -> io::Result<()> {
    let mut archive = ZipArchive::new(BufReader::new(archive)).map_err(io::Error::other)?;
    for index in 0..archive.len() {
        let entry = archive.by_index_raw(index).map_err(io::Error::other)?;
        let (kind, mode) = zip_kind(&entry);
        let locked = entry.encrypted();
        let path = entry.name_raw().to_vec();
        let size = entry.size();
        let (modified, clock) = zip_modified(&entry);
        drop(entry);
        let mut link = Vec::new();
        if kind == Kind::Link && !locked {
            archive
                .by_index(index)
                .map_err(io::Error::other)?
                .take(MAX_LINK)
                .read_to_end(&mut link)?;
        }
        wire::write(
            out,
            &Entry {
                kind,
                locked,
                size,
                modified,
                clock,
                mode,
                path: &path,
                link: &link,
            },
        )?;
    }
    Ok(())
}

pub fn zip_kind<R: Read>(entry: &zip::read::ZipFile<'_, R>) -> (Kind, u32) {
    let mode = entry.unix_mode();
    let kind = if entry.is_dir() {
        Kind::Folder
    } else if mode.is_some_and(|mode| mode & TYPE_BITS == LINK_TYPE) {
        Kind::Link
    } else {
        Kind::File
    };
    let mode = mode.unwrap_or(if kind == Kind::Folder {
        FOLDER_MODE
    } else {
        FILE_MODE
    });
    (kind, mode)
}

pub fn zip_modified<R: Read>(entry: &zip::read::ZipFile<'_, R>) -> (i64, Clock) {
    let unix = entry.extra_data_fields().find_map(|field| match field {
        ExtraField::ExtendedTimestamp(stamp) => stamp.mod_time(),
        _ => None,
    });
    if let Some(unix) = unix {
        return (i64::from(unix), Clock::Utc);
    }
    let wall = chrono::NaiveDateTime::try_from(entry.last_modified().unwrap_or_default())
        .or_else(|_| chrono::NaiveDateTime::try_from(zip::DateTime::default()))
        .map_or(0, |time| time.and_utc().timestamp());
    (wall, Clock::Local)
}

fn list_tar(stream: impl Read, out: &mut impl Write) -> io::Result<()> {
    let mut archive = tar::Archive::new(stream);
    archive.set_ignore_zeros(true);
    for entry in archive.entries()? {
        let entry = entry?;
        let header = entry.header();
        let path = entry.path_bytes();
        let Some(kind) = tar_kind(header.entry_type(), &path) else {
            continue;
        };
        let link = entry.link_name_bytes().unwrap_or_default();
        wire::write(
            out,
            &Entry {
                kind,
                locked: false,
                size: entry.size(),
                modified: header
                    .mtime()
                    .map_or(0, |seconds| i64::try_from(seconds).unwrap_or(i64::MAX)),
                clock: Clock::Utc,
                mode: header.mode().unwrap_or(FILE_MODE),
                path: &path,
                link: &link,
            },
        )?;
    }
    Ok(())
}

pub fn tar_kind(kind: tar::EntryType, path: &[u8]) -> Option<Kind> {
    if kind.is_dir() || (kind.is_file() && path.ends_with(b"/")) {
        Some(Kind::Folder)
    } else if kind.is_symlink() {
        Some(Kind::Link)
    } else if kind.is_file() || kind.is_hard_link() || kind.is_contiguous() || kind.is_gnu_sparse()
    {
        Some(Kind::File)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use tar::EntryType;

    use super::*;

    #[test]
    fn tells_tar_kinds() {
        assert_eq!(tar_kind(EntryType::Regular, b"a"), Some(Kind::File));
        assert_eq!(tar_kind(EntryType::Regular, b"old/"), Some(Kind::Folder));
        assert_eq!(tar_kind(EntryType::Directory, b"d"), Some(Kind::Folder));
        assert_eq!(tar_kind(EntryType::Symlink, b"l"), Some(Kind::Link));
        assert_eq!(tar_kind(EntryType::Link, b"h"), Some(Kind::File));
        assert_eq!(tar_kind(EntryType::Continuous, b"c"), Some(Kind::File));
        assert_eq!(tar_kind(EntryType::GNUSparse, b"s"), Some(Kind::File));
        assert_eq!(tar_kind(EntryType::Char, b"tty"), None);
        assert_eq!(tar_kind(EntryType::Fifo, b"p"), None);
    }
}
