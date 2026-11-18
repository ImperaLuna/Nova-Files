//! Makes an archive of files and folders. Each source goes in under its
//! own name, a folder with all it holds; links go in as links, never
//! followed; sockets, pipes and devices are left out. For each file it
//! writes a line to the core: the bytes the file held, in decimal.

use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::Path;

use chrono::{Datelike as _, Timelike as _};
use zip::write::{FullFileOptions, ZipWriter};
use zip::{AesMode, CompressionMethod};

use crate::ending::Ending;
use crate::progress::{Told, Unheard};

/// What the archive is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Packing {
    ZipDeflate,
    ZipZstd,
    TarZstd,
    TarGzip,
    TarXz,
}

impl Packing {
    /// The packing the core names `word`.
    pub fn of(word: &OsStr) -> Option<Self> {
        Some(match word.as_bytes() {
            b"zip-deflate" => Self::ZipDeflate,
            b"zip-zstd" => Self::ZipZstd,
            b"tar-zst" => Self::TarZstd,
            b"tar-gz" => Self::TarGzip,
            b"tar-xz" => Self::TarXz,
            _ => return None,
        })
    }
}

/// How hard to compress, from [`FASTEST`] to [`SMALLEST`].
pub type Level = u32;
pub const FASTEST: Level = 1;
pub const SMALLEST: Level = 9;
/// Zstandard's own levels for each of ours: its top levels are slow for
/// little gain, so ours stop at 19.
const ZSTD_LEVELS: [i32; 9] = [1, 3, 5, 7, 9, 12, 15, 17, 19];

const FOLDER_TYPE: u32 = 0o040_000;
const LINK_TYPE: u32 = 0o120_000;
const FILE_TYPE: u32 = 0o100_000;
/// A zip entry's Unix mode sits in the top half of its external
/// attributes; the bottom half holds DOS attributes.
const UNIX_SHIFT: u32 = 16;
const DOS_FOLDER: u32 = 0x10;
const PERMISSION_BITS: u32 = 0o7777;
const PART: &[u8] = b"/";

enum Stopped {
    Read(io::Error),
    Write(io::Error),
    Unheard,
}

impl From<Unheard> for Stopped {
    fn from(Unheard: Unheard) -> Self {
        Self::Unheard
    }
}

/// How to make an archive.
pub struct Recipe<'a> {
    pub packing: Packing,
    pub level: Level,
    /// Locks each zip file entry.
    pub password: Option<&'a str>,
}

/// Writes `sources` into `output` as `recipe` says.
pub fn compress(
    output: File,
    sources: &[&Path],
    recipe: &Recipe<'_>,
    out: &mut impl Write,
) -> Ending {
    let mut told = Told::new(out);
    let packing = recipe.packing;
    let level = recipe.level.clamp(FASTEST, SMALLEST);
    let Ok(made) = output.metadata() else {
        return Ending::NotWritten;
    };
    let itself = (made.dev(), made.ino());
    let output = BufWriter::new(output);
    let result = match packing {
        Packing::ZipDeflate | Packing::ZipZstd => {
            zip_all(output, sources, itself, recipe, &mut told)
        }
        Packing::TarZstd => zstd::Encoder::new(output, zstd_level(level))
            .map_err(Stopped::Write)
            .and_then(|encoder| {
                let encoder = tar_all(encoder, sources, itself, &mut told)?;
                encoder
                    .finish()
                    .map_err(Stopped::Write)?
                    .flush()
                    .map_err(Stopped::Write)
            }),
        Packing::TarGzip => {
            let encoder = flate2::write::GzEncoder::new(output, flate2::Compression::new(level));
            tar_all(encoder, sources, itself, &mut told).and_then(|encoder| {
                encoder
                    .finish()
                    .map_err(Stopped::Write)?
                    .flush()
                    .map_err(Stopped::Write)
            })
        }
        Packing::TarXz => {
            lzma_rust2::XzWriter::new(output, lzma_rust2::XzOptions::with_preset(level))
                .map_err(Stopped::Write)
                .and_then(|encoder| {
                    let encoder = tar_all(encoder, sources, itself, &mut told)?;
                    encoder
                        .finish()
                        .map_err(Stopped::Write)?
                        .flush()
                        .map_err(Stopped::Write)
                })
        }
    };
    match result {
        Ok(()) => Ending::Done,
        Err(Stopped::Read(error)) => Ending::of_open(&error),
        Err(Stopped::Write(error)) => Ending::of_write(&error),
        Err(Stopped::Unheard) => Ending::Usage,
    }
}

fn zstd_level(level: Level) -> i32 {
    let index = usize::try_from(level - FASTEST).unwrap_or_default();
    ZSTD_LEVELS.get(index).copied().unwrap_or_default()
}

/// The device and inode of the archive being made, which no walk takes.
type Itself = (u64, u64);

/// Calls `each` with every entry under `source`, `source` first, a
/// folder before what it holds, each with its name in the archive; the
/// archive being made is left out.
fn walk(
    source: &Path,
    itself: Itself,
    each: &mut impl FnMut(&Path, &[u8], &fs::Metadata) -> Result<(), Stopped>,
) -> Result<(), Stopped> {
    walk_in(
        source,
        source.file_name().unwrap_or_default().as_bytes(),
        itself,
        each,
    )
}

fn walk_in(
    path: &Path,
    name: &[u8],
    itself: Itself,
    each: &mut impl FnMut(&Path, &[u8], &fs::Metadata) -> Result<(), Stopped>,
) -> Result<(), Stopped> {
    let metadata = path.symlink_metadata().map_err(Stopped::Read)?;
    if (metadata.dev(), metadata.ino()) == itself {
        return Ok(());
    }
    each(path, name, &metadata)?;
    if !metadata.is_dir() {
        return Ok(());
    }
    let mut inside: Vec<_> = fs::read_dir(path)
        .map_err(Stopped::Read)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<_, _>>()
        .map_err(Stopped::Read)?;
    inside.sort();
    for child in inside {
        let child_name = [name, PART, child.as_bytes()].concat();
        walk_in(&path.join(&child), &child_name, itself, each)?;
    }
    Ok(())
}

fn tar_all<W: Write>(
    output: W,
    sources: &[&Path],
    itself: Itself,
    told: &mut Told<'_, impl Write>,
) -> Result<W, Stopped> {
    let mut builder = tar::Builder::new(output);
    builder.follow_symlinks(false);
    builder.mode(tar::HeaderMode::Complete);
    for source in sources {
        walk(source, itself, &mut |path, name, metadata| {
            let kind = metadata.file_type();
            if !(kind.is_file() || kind.is_dir() || kind.is_symlink()) {
                return Ok(());
            }
            builder
                .append_path_with_name(path, OsStr::from_bytes(name))
                .map_err(|error| sort(error, path))?;
            told.done(if kind.is_file() { metadata.len() } else { 0 })
                .map_err(Stopped::from)
        })?;
    }
    builder.into_inner().map_err(Stopped::Write)
}

/// A tar error at `path`: reading it when `path` can't be opened now,
/// else writing.
fn sort(error: io::Error, path: &Path) -> Stopped {
    if File::open(path).is_err() {
        Stopped::Read(error)
    } else {
        Stopped::Write(error)
    }
}

fn zip_all(
    output: impl Write + io::Seek,
    sources: &[&Path],
    itself: Itself,
    recipe: &Recipe<'_>,
    told: &mut Told<'_, impl Write>,
) -> Result<(), Stopped> {
    let (packing, password) = (recipe.packing, recipe.password);
    let level = recipe.level.clamp(FASTEST, SMALLEST);
    let mut zip = ZipWriter::new(output).set_auto_large_file();
    let (method, level) = if packing == Packing::ZipZstd {
        (CompressionMethod::Zstd, i64::from(zstd_level(level)))
    } else {
        (CompressionMethod::Deflated, i64::from(level))
    };
    for source in sources {
        walk(source, itself, &mut |path, name, metadata| {
            let kind = metadata.file_type();
            let mode = metadata.permissions().mode() & PERMISSION_BITS;
            let base = FullFileOptions::default()
                .last_modified_time(dos_time(metadata.mtime()))
                .with_extended_timestamp(u32::try_from(metadata.mtime()).ok(), None, None)
                .map_err(|error| Stopped::Write(io::Error::other(error)))?;
            let base = match password {
                Some(password) if kind.is_file() => {
                    base.with_aes_encryption(AesMode::Aes256, password)
                }
                _ => base,
            };
            let to_zip = |error| Stopped::Write(io::Error::other(error));
            if kind.is_dir() {
                let name = [name, PART].concat();
                let options = base
                    .name(&name)
                    .compression_method(CompressionMethod::Stored)
                    .external_attributes(((FOLDER_TYPE + mode) << UNIX_SHIFT) + DOS_FOLDER);
                zip.start_file_with_options(options).map_err(to_zip)?;
                return told.done(0).map_err(Stopped::from);
            }
            if kind.is_symlink() {
                let target = fs::read_link(path).map_err(Stopped::Read)?;
                let options = base
                    .name(name)
                    .compression_method(CompressionMethod::Stored)
                    .external_attributes((LINK_TYPE + mode) << UNIX_SHIFT);
                zip.start_file_with_options(options).map_err(to_zip)?;
                zip.write_all(target.as_os_str().as_bytes())
                    .map_err(Stopped::Write)?;
                return told.done(0).map_err(Stopped::from);
            }
            if !kind.is_file() {
                return Ok(());
            }
            let mut file = File::open(path).map_err(Stopped::Read)?;
            let options = base
                .name(name)
                .compression_method(method)
                .compression_level(Some(level))
                .external_attributes((FILE_TYPE + mode) << UNIX_SHIFT);
            zip.start_file_with_options(options).map_err(to_zip)?;
            let bytes = copy(&mut file, &mut zip)?;
            told.done(bytes).map_err(Stopped::from)
        })?;
    }
    zip.finish()
        .map_err(|error| Stopped::Write(io::Error::other(error)))?
        .flush()
        .map_err(Stopped::Write)
}

fn copy(from: &mut impl Read, to: &mut impl Write) -> Result<u64, Stopped> {
    let mut buffer = vec![0; crate::unpack::BUFFER_SIZE];
    let mut copied = 0;
    loop {
        let read = match from.read(&mut buffer) {
            Ok(0) => return Ok(copied),
            Ok(read) => read,
            Err(error) => return Err(Stopped::Read(error)),
        };
        to.write_all(&buffer[..read]).map_err(Stopped::Write)?;
        copied += read as u64;
    }
}

/// `seconds` since 1970 as a zip's DOS time: wall time in the user's
/// zone, 1980 at the earliest.
fn dos_time(seconds: i64) -> zip::DateTime {
    chrono::DateTime::from_timestamp(seconds, 0)
        .map(|time| time.with_timezone(&chrono::Local).naive_local())
        .and_then(|wall| {
            zip::DateTime::from_date_and_time(
                u16::try_from(wall.year()).ok()?,
                u8::try_from(wall.month()).ok()?,
                u8::try_from(wall.day()).ok()?,
                u8::try_from(wall.hour()).ok()?,
                u8::try_from(wall.minute()).ok()?,
                u8::try_from(wall.second()).ok()?,
            )
            .ok()
        })
        .unwrap_or_default()
}
