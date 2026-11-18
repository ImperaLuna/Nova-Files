use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Component, Path, PathBuf};

use memmap2::Mmap;

use crate::format::{self, HEADER_LEN, Header, MAGIC, NO_PARENT, ROW_LEN, Row, VERSION};
use crate::{Entry, Flags, Kind, RowId, Stamp};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenError {
    Missing,
    NotAnIndex,
    OutOfDate,
    Damaged(&'static str),
    Failed(io::ErrorKind),
}

impl From<io::Error> for OpenError {
    fn from(error: io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::NotFound => Self::Missing,
            kind => Self::Failed(kind),
        }
    }
}

#[derive(Debug)]
pub struct Index {
    map: Mmap,
    header: Header,
}

impl Index {
    pub fn open(path: &Path) -> Result<Self, OpenError> {
        let file = File::open(path)?;
        #[expect(unsafe_code, reason = "mapping the index is the point of this crate")]
        // SAFETY: the map is only sound while nobody changes the file under
        // it. Nova Files never writes an index in place: `Built::save`
        // writes a new file and renames it over this one, which leaves this
        // file, and the map, as they were. The file lives in the user's
        // own cache folder (mode 0700), so only the user's own programs can
        // reach it; one that truncates it would end this process with
        // SIGBUS, as with any mapped file.
        let map = unsafe { Mmap::map(&file)? };
        let header = check(&map)?;
        Ok(Self { map, header })
    }

    pub const fn rows(&self) -> u32 {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "check() refused more rows than a u32 counts"
        )]
        let rows = self.header.rows as u32;
        rows
    }

    pub fn bytes(&self) -> usize {
        self.map.len()
    }

    pub const fn crawled(&self) -> Option<Stamp> {
        format::stamp(self.header.crawled)
    }

    pub fn root(&self) -> &Path {
        Path::new(self.name(RowId(0)).unwrap_or_default())
    }

    pub fn entry(&self, row: RowId) -> Option<Entry> {
        let raw = self.raw(row)?;
        Some(Entry {
            parent: raw.parent(),
            end: RowId(raw.end),
            kind: raw.kind.unwrap_or(Kind::File),
            flags: Flags(raw.flags),
            id: raw.id,
            size: raw.size,
            modified: format::stamp(raw.modified),
            created: format::stamp(raw.created),
        })
    }

    pub fn name(&self, row: RowId) -> Option<&OsStr> {
        let raw = self.raw(row)?;
        let names = self.map.get(names_start(&self.header)?..)?;
        let at = usize::try_from(raw.name_at).ok()?;
        let name = names.get(at..at + usize::from(raw.name_len))?;
        Some(OsStr::from_bytes(name))
    }

    pub fn children(&self, folder: RowId) -> impl Iterator<Item = RowId> + '_ {
        let end = self.entry(folder).map_or(folder, |entry| entry.end);
        let first = RowId(folder.0.saturating_add(1));
        std::iter::successors(Some(first).filter(|&row| row < end), move |&row| {
            self.entry(row)
                .map(|entry| entry.end)
                .filter(|&next| next < end)
        })
    }

    pub fn row_of(&self, path: &Path) -> Option<RowId> {
        let inside = path.strip_prefix(self.root()).ok()?;
        let mut row = RowId(0);
        for part in inside.components() {
            let Component::Normal(name) = part else {
                return None;
            };
            row = self
                .children(row)
                .find(|&child| self.name(child) == Some(name))?;
        }
        Some(row)
    }

    pub fn path(&self, row: RowId) -> Option<PathBuf> {
        let mut names = Vec::new();
        let mut next = Some(row);
        while let Some(at) = next {
            names.push(self.name(at)?);
            next = self.raw(at)?.parent();
            if next.is_some_and(|parent| parent >= at) {
                return None;
            }
        }
        Some(names.iter().rev().collect())
    }

    fn raw(&self, RowId(row): RowId) -> Option<Row> {
        let row = usize::try_from(row).ok()?;
        if row >= usize::try_from(self.header.rows).ok()? {
            return None;
        }
        Row::read(self.map.get(HEADER_LEN + row * ROW_LEN..)?)
    }
}

fn names_start(header: &Header) -> Option<usize> {
    usize::try_from(header.rows)
        .ok()?
        .checked_mul(ROW_LEN)?
        .checked_add(HEADER_LEN)
}

fn check(bytes: &[u8]) -> Result<Header, OpenError> {
    let header = Header::read(bytes).ok_or(OpenError::Damaged("shorter than a header"))?;
    if header.magic != MAGIC {
        return Err(OpenError::NotAnIndex);
    }
    if header.version != VERSION || usize::try_from(header.row_len) != Ok(ROW_LEN) {
        return Err(OpenError::OutOfDate);
    }
    if header.rows == 0 || header.rows > u64::from(NO_PARENT) {
        return Err(OpenError::Damaged("row count out of range"));
    }
    let expected = names_start(&header)
        .zip(usize::try_from(header.names_len).ok())
        .and_then(|(start, names)| start.checked_add(names));
    if expected != Some(bytes.len()) {
        return Err(OpenError::Damaged("length doesn't match the header"));
    }
    check_rows(bytes, &header)?;
    Ok(header)
}

fn check_rows(bytes: &[u8], header: &Header) -> Result<(), OpenError> {
    let rows = names_start(header)
        .and_then(|end| bytes.get(HEADER_LEN..end))
        .ok_or(OpenError::Damaged("length doesn't match the header"))?;
    let mut open: Vec<(u32, u32)> = Vec::new();
    for (raw, at) in rows
        .as_chunks::<ROW_LEN>()
        .0
        .iter()
        .map_while(|row| Row::read(row))
        .zip(0..)
    {
        check_name(&raw, header, at)?;
        while open.last().is_some_and(|&(_, end)| end <= at) {
            open.pop();
        }
        let enclosing = open.last().copied();
        let parent = enclosing.map_or(NO_PARENT, |(folder, _)| folder);
        if raw.parent != parent {
            return Err(OpenError::Damaged(
                "a row's parent isn't the folder around it",
            ));
        }
        let limit = enclosing.map_or(header.rows, |(_, end)| u64::from(end));
        check_end(&raw, at, limit)?;
        if raw.kind == Some(Kind::Folder) {
            open.push((at, raw.end));
        }
    }
    Ok(())
}

fn check_name(raw: &Row, header: &Header, at: u32) -> Result<(), OpenError> {
    if raw.kind.is_none() {
        return Err(OpenError::Damaged("a row of no known kind"));
    }
    if at == 0 && raw.kind != Some(Kind::Folder) {
        return Err(OpenError::Damaged("the first row isn't a folder"));
    }
    let name_end = u64::from(raw.name_at) + u64::from(raw.name_len);
    if name_end > header.names_len {
        return Err(OpenError::Damaged("a name runs past the names block"));
    }
    Ok(())
}

fn check_end(raw: &Row, at: u32, limit: u64) -> Result<(), OpenError> {
    let end = u64::from(raw.end);
    let next = u64::from(at) + 1;
    let fits = end >= next && end <= limit;
    let leaf_ok = raw.kind == Some(Kind::Folder) || end == next;
    let root_ok = at != 0 || end == limit;
    if fits && leaf_ok && root_ok {
        Ok(())
    } else {
        Err(OpenError::Damaged("a row's run doesn't fit its parent's"))
    }
}

#[cfg(test)]
mod tests;
