//! Unpacks an archive into the folder it is given. For each entry done it
//! writes a line to the core: the bytes the entry held, in decimal.

use std::fs::File;
use std::io::{self, BufReader, Read, Write};

use chrono::TimeZone as _;
use zip::ZipArchive;
use zip::result::ZipError;

use crate::decompress::tar_stream;
use crate::ending::Ending;
use crate::format::{self, Format};
use crate::list::{FILE_MODE, MAX_LINK, tar_kind, zip_kind, zip_modified};
use crate::unpack::{Stamp, Unpacking, WriteError, parts};
use crate::wire::{Clock, Kind};

enum Stopped {
    Read,
    Write(io::Error),
    /// The core stopped reading.
    Unheard,
    NeedsPassword,
    WrongPassword,
}

impl From<WriteError> for Stopped {
    fn from(error: WriteError) -> Self {
        Self::Write(error.0)
    }
}

/// Unpacks every entry of `archive` into `into`, opening locked entries
/// with `password`.
pub fn extract(
    mut archive: File,
    into: Unpacking,
    password: Option<&[u8]>,
    out: &mut impl Write,
) -> Ending {
    let mut told = Told { out };
    let result = match format::sniff(&mut archive) {
        Ok(Some(Format::Zip)) => extract_zip(archive, into, password, &mut told),
        Ok(Some(Format::Tar(compression))) => tar_stream(archive, compression)
            .map_or(Err(Stopped::Read), |stream| {
                extract_tar(stream, into, &mut told)
            }),
        Ok(None) => return Ending::Unsupported,
        Err(_) => return Ending::Broken,
    };
    match result {
        Ok(()) => Ending::Done,
        Err(Stopped::Read) => Ending::Broken,
        Err(Stopped::Write(error)) => Ending::of_write(&error),
        Err(Stopped::Unheard) => Ending::Usage,
        Err(Stopped::NeedsPassword) => Ending::NeedsPassword,
        Err(Stopped::WrongPassword) => Ending::WrongPassword,
    }
}

struct Told<'a, W> {
    out: &'a mut W,
}

impl<W: Write> Told<'_, W> {
    fn done(&mut self, bytes: u64) -> Result<(), Stopped> {
        writeln!(self.out, "{bytes}")
            .and_then(|()| self.out.flush())
            .map_err(|_| Stopped::Unheard)
    }
}

fn extract_zip(
    archive: File,
    mut into: Unpacking,
    password: Option<&[u8]>,
    told: &mut Told<'_, impl Write>,
) -> Result<(), Stopped> {
    let mut archive = ZipArchive::new(BufReader::new(archive)).map_err(|_| Stopped::Read)?;
    for index in 0..archive.len() {
        let entry = archive.by_index_raw(index).map_err(|_| Stopped::Read)?;
        let (kind, mode) = zip_kind(&entry);
        let (modified, clock) = zip_modified(&entry);
        let locked = entry.encrypted();
        let name = entry.name_raw().to_vec();
        drop(entry);
        let Some(parts) = parts(&name) else {
            continue;
        };
        let stamp = Stamp {
            mode,
            modified: utc(modified, clock),
        };
        if kind == Kind::Folder {
            into.folder(&parts, stamp)?;
            told.done(0)?;
            continue;
        }
        let opened = match (locked, password) {
            (false, _) => archive.by_index(index),
            (true, Some(password)) => archive.by_index_decrypt(index, password),
            (true, None) => return Err(Stopped::NeedsPassword),
        };
        let mut entry = opened.map_err(|error| match error {
            ZipError::InvalidPassword => Stopped::WrongPassword,
            _ => Stopped::Read,
        })?;
        let unreadable = |_| {
            if locked {
                Stopped::WrongPassword
            } else {
                Stopped::Read
            }
        };
        let bytes = if kind == Kind::Link {
            let mut target = Vec::new();
            (&mut entry)
                .take(MAX_LINK)
                .read_to_end(&mut target)
                .map_err(unreadable)?;
            into.link(&parts, &target)?;
            0
        } else {
            into.file(&parts, stamp, &mut entry)?.map_err(unreadable)?
        };
        told.done(bytes)?;
    }
    into.finish()?;
    Ok(())
}

fn extract_tar(
    stream: impl Read,
    mut into: Unpacking,
    told: &mut Told<'_, impl Write>,
) -> Result<(), Stopped> {
    let mut archive = tar::Archive::new(stream);
    archive.set_ignore_zeros(true);
    for entry in archive.entries().map_err(|_| Stopped::Read)? {
        let mut entry = entry.map_err(|_| Stopped::Read)?;
        let path = entry.path_bytes().into_owned();
        let header = entry.header();
        let entry_type = header.entry_type();
        let Some(kind) = tar_kind(entry_type, &path) else {
            continue;
        };
        let Some(parts) = parts(&path) else {
            continue;
        };
        let stamp = Stamp {
            mode: header.mode().unwrap_or(FILE_MODE),
            modified: header
                .mtime()
                .map_or(0, |seconds| i64::try_from(seconds).unwrap_or(i64::MAX)),
        };
        let link = entry.link_name_bytes().unwrap_or_default().into_owned();
        let bytes = match kind {
            Kind::Folder => {
                into.folder(&parts, stamp)?;
                0
            }
            Kind::Link => {
                into.link(&parts, &link)?;
                0
            }
            Kind::File if entry_type.is_hard_link() => {
                if let Some(target) = self::parts(&link) {
                    into.hard_link(&parts, &target)?;
                }
                0
            }
            Kind::File => into
                .file(&parts, stamp, &mut entry)?
                .map_err(|_| Stopped::Read)?,
        };
        told.done(bytes)?;
    }
    into.finish()?;
    Ok(())
}

/// `seconds` as UTC; local wall time is read in the user's zone, the
/// earlier of two when the clocks went back.
fn utc(seconds: i64, clock: Clock) -> i64 {
    match clock {
        Clock::Utc => seconds,
        Clock::Local => chrono::DateTime::from_timestamp(seconds, 0)
            .and_then(|wall| {
                chrono::Local
                    .from_local_datetime(&wall.naive_utc())
                    .earliest()
            })
            .map_or(seconds, |time| time.timestamp()),
    }
}
