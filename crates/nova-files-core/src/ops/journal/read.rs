//! Reading a journal back after a crash: its lines, each a record word
//! and the paths it is about. A last line the crash cut short (no
//! newline) is left out, as its path may be cut short too.

use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use std::ffi::OsStr;

use super::{ESCAPE, HEADER, HIGH};

/// Radix of the two digits after an escape.
const HEX_RADIX: u32 = 16;

/// One whole line of a journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub word: String,
    pub paths: Vec<PathBuf>,
}

/// The whole lines of the journal at `path`, after its header.
///
/// # Errors
/// When it can't be read, or isn't a journal this version wrote.
pub fn lines(path: &Path) -> io::Result<Vec<Line>> {
    let bytes = fs::read(path)?;
    let whole = bytes
        .iter()
        .rposition(|&byte| byte == b'\n')
        .map_or(&bytes[..0], |end| &bytes[..end]);
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
            bytes.push(u8::try_from(high << HIGH | low).ok()?);
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
