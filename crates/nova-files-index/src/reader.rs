//! Reading a saved index through a memory map.
//!
//! [`Index::open`] maps the file and checks all of it once: the header, the
//! length, and that the rows form one tree in depth-first order (each row's
//! parent is the folder whose run it sits in, and every run sits inside its
//! parent's). After that, lookups only read the map. The kernel pages the
//! file in as it is read, so a search touches only the names block.

use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use memmap2::Mmap;

use crate::format::{self, HEADER_LEN, Header, MAGIC, NO_PARENT, ROW_LEN, Row, VERSION};
use crate::{Entry, Flags, Kind, RowId, Stamp};

/// Why [`Index::open`] gave no index. Each one means: crawl again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenError {
    /// There is no index yet.
    Missing,
    /// The file isn't an index.
    NotAnIndex,
    /// An index from another version of Nova Files.
    OutOfDate,
    /// The file is cut short or its rows don't form a tree, for example
    /// after a crash while it was written. Says what failed first.
    Damaged(&'static str),
    /// The file couldn't be read.
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

/// A saved index, mapped and checked.
#[derive(Debug)]
pub struct Index {
    map: Mmap,
    header: Header,
}

impl Index {
    /// Maps and checks the index at `path`.
    ///
    /// # Errors
    /// [`OpenError`] when there is no usable index there.
    pub fn open(path: &Path) -> Result<Self, OpenError> {
        let file = File::open(path)?;
        if usize::try_from(file.metadata()?.len()).is_ok_and(|len| len < HEADER_LEN) {
            return Err(OpenError::Damaged("shorter than a header"));
        }
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

    /// How many rows it has; always at least one, the crawled folder.
    pub const fn len(&self) -> u32 {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "check() refused more rows than a u32 counts"
        )]
        let rows = self.header.rows as u32;
        rows
    }

    /// Never true: an index always holds the crawled folder.
    pub const fn is_empty(&self) -> bool {
        self.header.rows == 0
    }

    /// The file's size in bytes.
    pub fn bytes(&self) -> usize {
        self.map.len()
    }

    /// When the crawl that built it started.
    pub const fn crawled(&self) -> Option<Stamp> {
        format::stamp(self.header.crawled)
    }

    /// The folder that was crawled: row 0.
    pub fn root(&self) -> &Path {
        Path::new(self.name(RowId(0)).unwrap_or_default())
    }

    /// The row `row`, or `None` past the end.
    pub fn entry(&self, row: RowId) -> Option<Entry> {
        let raw = self.raw(row)?;
        Some(Entry {
            parent: raw.parent(),
            end: RowId(raw.end),
            // check() refused rows without a kind.
            kind: raw.kind.unwrap_or(Kind::File),
            flags: Flags(raw.flags),
            id: raw.id,
            size: raw.size,
            modified: format::stamp(raw.modified),
            created: format::stamp(raw.created),
        })
    }

    /// The name of `row`: the crawled folder's whole path for row 0, a
    /// single name for every other row. Any bytes, not only UTF-8.
    pub fn name(&self, row: RowId) -> Option<&OsStr> {
        let raw = self.raw(row)?;
        let names = self.map.get(names_start(&self.header)?..)?;
        let at = usize::try_from(raw.name_at).ok()?;
        let name = names.get(at..at + usize::from(raw.name_len))?;
        Some(OsStr::from_bytes(name))
    }

    /// The full path of `row`, put together from its parents' names.
    pub fn path(&self, row: RowId) -> Option<PathBuf> {
        let mut names = Vec::new();
        let mut next = Some(row);
        while let Some(at) = next {
            names.push(self.name(at)?);
            next = self.raw(at)?.parent();
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

/// Checks a whole index file and returns its header.
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

/// Checks that the rows form one tree in depth-first order, with names
/// inside the names block.
fn check_rows(bytes: &[u8], header: &Header) -> Result<(), OpenError> {
    let rows = names_start(header)
        .and_then(|end| bytes.get(HEADER_LEN..end))
        .ok_or(OpenError::Damaged("length doesn't match the header"))?;
    // The folders whose runs the current row is inside, innermost last,
    // with where each run ends.
    let mut open: Vec<(u32, u32)> = Vec::new();
    for (raw, at) in rows.chunks_exact(ROW_LEN).map_while(Row::read).zip(0..) {
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

/// A row's run starts after it, stays inside `limit` (its parent's run, or
/// the whole index for row 0) and is just the row for anything but a
/// folder. Row 0's run is the whole index.
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
