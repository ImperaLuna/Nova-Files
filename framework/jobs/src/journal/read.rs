use std::fs::File;
use std::io::{self, Read as _};
use std::ops::Range;
use std::path::{Path, PathBuf};

use super::{HEADER, entry_from, word};
use nova_files_disk::percent::unescape;

const MAX_BYTES: u64 = 268_435_456;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub word: String,
    pub paths: Vec<PathBuf>,
}

#[derive(Debug)]
pub struct Written {
    pub lines: Vec<Line>,
    pub whole: u64,
}

pub fn read(file: &File, path: &Path) -> io::Result<Written> {
    read_at_most(file, path, MAX_BYTES)
}

pub fn read_at_most(file: &File, path: &Path, limit: u64) -> io::Result<Written> {
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

fn parse(whole: &[u8], path: &Path) -> io::Result<Vec<Line>> {
    let mut lines = whole.split(|&byte| byte == b'\n');
    if lines.next() != Some(HEADER.as_bytes()) {
        return Err(not_a_journal(path));
    }
    lines
        .enumerate()
        .map(|(at, line)| {
            let mut fields = line.split(|&byte| byte == b' ');
            let word = fields
                .next()
                .and_then(|word| std::str::from_utf8(word).ok());
            let paths: Option<Vec<_>> = fields.map(unescape).collect();
            match (word, paths) {
                (Some(word), Some(paths)) if fits(word, &paths, at == 0) => Ok(Line {
                    word: word.to_owned(),
                    paths,
                }),
                _ => Err(not_a_journal(path)),
            }
        })
        .collect()
}

const PLANNED_ENTRIES: Range<usize> = 1..2;
const TRASH_INFO_ENTRIES: Range<usize> = 1..3;

fn fits(name: &str, fields: &[PathBuf], first: bool) -> bool {
    if word::fields(name).is_none() {
        return first && fields.is_empty() && !name.is_empty();
    }
    let entries = match name {
        word::PLANNED => PLANNED_ENTRIES,
        word::TRASH_INFO => TRASH_INFO_ENTRIES,
        _ => 0..0,
    };
    let entry_fits = fields
        .get(entries)
        .is_some_and(|entries| entries.iter().all(|field| entry_from(field).is_some()));
    word::fields(name) == Some(fields.len()) && entry_fits
}

fn not_a_journal(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{} isn't a job journal", path.display()),
    )
}
