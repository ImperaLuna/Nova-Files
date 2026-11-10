//! Reading a journal back after a crash: its lines, each a record word
//! and the paths it is about. A last line the crash cut short (no
//! newline) is left out, as its path may be cut short too.

use std::fs::File;
use std::io::{self, Read as _};
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use std::ffi::OsStr;

use super::{ESCAPE, HEADER, HIGH};

/// Radix of the two digits after an escape.
const HEX_RADIX: u32 = 16;
/// The most a journal may hold to be read. A job writes far less; a file
/// bigger than this is left alone rather than read into memory.
/// 256 MiB.
const MAX_BYTES: u64 = 268_435_456;

/// One whole line of a journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub word: String,
    pub paths: Vec<PathBuf>,
}

/// A journal read back: its whole lines after the header, and how many
/// bytes they take, header included.
#[derive(Debug)]
pub struct Written {
    pub lines: Vec<Line>,
    pub whole: u64,
}

/// The journal at `path`, open as `file`, read from its start.
///
/// # Errors
/// When it can't be read, or isn't a journal this version wrote.
pub fn read(file: &File, path: &Path) -> io::Result<Written> {
    read_at_most(file, path, MAX_BYTES)
}

/// [`read`], refusing a journal over `limit` bytes.
///
/// # Errors
/// When it can't be read, is too big, or isn't a journal this version
/// wrote.
pub(super) fn read_at_most(file: &File, path: &Path, limit: u64) -> io::Result<Written> {
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            format!("{} is over {limit} bytes", path.display()),
        ));
    }
    let end = bytes.iter().rposition(|&byte| byte == b'\n');
    let whole = end.map_or(&bytes[..0], |end| &bytes[..end]);
    let lines = parse(whole, path)?;
    let whole = end.map_or(0, |end| end + 1);
    Ok(Written {
        lines,
        whole: u64::try_from(whole).unwrap_or(u64::MAX),
    })
}

/// The lines of `whole`, a journal's whole lines without the last
/// newline, after its header.
fn parse(whole: &[u8], path: &Path) -> io::Result<Vec<Line>> {
    let mut lines = whole.split(|&byte| byte == b'\n');
    if lines.next() != Some(HEADER.as_bytes()) {
        return Err(not_a_journal(path));
    }
    lines
        .map(|line| {
            let mut fields = line.split(|&byte| byte == b' ');
            let word = fields
                .next()
                .and_then(|word| std::str::from_utf8(word).ok());
            let paths: Option<Vec<_>> = fields.map(unescape).collect();
            match (word, paths) {
                (Some(word), Some(paths)) => Ok(Line {
                    word: word.to_owned(),
                    paths,
                }),
                _ => Err(not_a_journal(path)),
            }
        })
        .collect()
}

/// The path `field` was escaped from, or `None` when it isn't escaped
/// right.
fn unescape(field: &[u8]) -> Option<PathBuf> {
    let mut bytes = Vec::with_capacity(field.len());
    let mut rest = field.iter();
    while let Some(&byte) = rest.next() {
        if byte == ESCAPE {
            let high = char::from(*rest.next()?).to_digit(HEX_RADIX)?;
            let low = char::from(*rest.next()?).to_digit(HEX_RADIX)?;
            bytes.push(u8::try_from((high << HIGH) + low).ok()?);
        } else {
            bytes.push(byte);
        }
    }
    Some(PathBuf::from(OsStr::from_bytes(&bytes)))
}

fn not_a_journal(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{} isn't a job journal", path.display()),
    )
}
