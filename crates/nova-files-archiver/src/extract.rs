//! Unpacks an archive, or some of its entries, into the folder it is
//! given. For each entry done it writes a line to the core: the bytes the
//! entry held, in decimal.

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::os::unix::ffi::OsStrExt as _;
use std::path::Path;

use chrono::TimeZone as _;
use zip::ZipArchive;
use zip::result::ZipError;

use crate::decompress::tar_stream;
use crate::ending::Ending;
use crate::format::{self, Format};
use crate::list::{FILE_MODE, MAX_LINK, tar_kind, zip_kind, zip_modified};
use crate::progress::{Told, Unheard};
use crate::seven::Seven;
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

impl From<Unheard> for Stopped {
    fn from(Unheard: Unheard) -> Self {
        Self::Unheard
    }
}

impl From<WriteError> for Stopped {
    fn from(error: WriteError) -> Self {
        Self::Write(error.0)
    }
}

/// Where an archive unpacks, and which of its entries.
pub struct Target<'a> {
    pub into: Unpacking,
    /// The folder `into` is.
    pub folder: &'a Path,
    /// The paths inside the archive to unpack, with all inside them;
    /// every entry when empty.
    pub picked: &'a [OsString],
}

/// Which entries to unpack: the parts of each picked path.
struct Picked<'a>(Vec<Vec<&'a [u8]>>);

impl<'a> Picked<'a> {
    /// `None` when a picked path has a `..` part.
    fn of(picked: &'a [OsString]) -> Option<Self> {
        picked
            .iter()
            .map(|path| parts(path.as_bytes()))
            .collect::<Option<_>>()
            .map(Self)
    }

    /// Whether the entry with these `parts` is one to unpack.
    fn wants(&self, parts: &[&[u8]]) -> bool {
        self.0.is_empty() || self.0.iter().any(|picked| parts.starts_with(picked))
    }
}

/// Unpacks the entries of `archive` that `target` picks, opening locked
/// entries with the password `seven` holds.
pub fn extract(
    mut archive: File,
    target: Target<'_>,
    seven: Seven<'_>,
    out: &mut impl Write,
) -> Ending {
    let Some(picked) = Picked::of(target.picked) else {
        return Ending::Usage;
    };
    let into = target.into;
    let result = match format::sniff(&mut archive) {
        Ok(Some(Format::Seven)) => return seven.extract(target.folder, target.picked, out),
        Ok(Some(Format::Zip)) => extract_zip(
            archive,
            (into, &picked),
            seven.password,
            &mut Told::new(out),
        ),
        Ok(Some(Format::Tar(compression))) => tar_stream(archive, compression)
            .map_or(Err(Stopped::Read), |stream| {
                extract_tar(stream, (into, &picked), &mut Told::new(out))
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

fn extract_zip(
    archive: File,
    (mut into, picked): (Unpacking, &Picked<'_>),
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
        let Some(parts) = parts(&name).filter(|parts| picked.wants(parts)) else {
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
    (mut into, picked): (Unpacking, &Picked<'_>),
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
        let Some(parts) = parts(&path).filter(|parts| picked.wants(parts)) else {
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
