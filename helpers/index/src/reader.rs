use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Component, Path, PathBuf};

use memmap2::{Mmap, UncheckedAdvice};

use crate::format::{self, DETAIL_LEN, Header, Layout, MAGIC, NO_PARENT, Row, VERSION};
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
    layout: Layout,
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
        let (header, layout) = check(&map)?;
        Ok(Self {
            map,
            header,
            layout,
        })
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
        let names = self.map.get(self.layout.names..)?;
        let at = usize::try_from(raw.name_at).ok()?;
        let name = names.get(at..at + usize::from(raw.name_len))?;
        Some(OsStr::from_bytes(name))
    }

    pub fn end(&self, row: RowId) -> Option<RowId> {
        self.word(self.layout.ends, row.0).map(RowId)
    }

    pub fn kind(&self, row: RowId) -> Option<Kind> {
        format::traits_kind(self.traits(row)?)
    }

    pub fn is_hidden(&self, row: RowId) -> bool {
        self.traits(row).is_some_and(format::traits_hidden)
    }

    pub fn folded(&self, row: RowId) -> Option<&[u8]> {
        let (start, end) = self.folded_span(row)?;
        self.map.get(self.layout.folded..)?.get(start..end)
    }

    pub fn starts_word(&self, row: RowId, at: usize) -> bool {
        self.folded_span(row).is_some_and(|(start, _)| {
            let bits = self.map.get(self.layout.word_starts..self.layout.details);
            format::bit_at(bits.unwrap_or_default(), start + at)
        })
    }

    pub fn release_details(&self) {
        let (at, len) = (self.layout.details, self.layout.len - self.layout.details);
        #[expect(unsafe_code, reason = "dropping pages of the map needs madvise")]
        // SAFETY: the map is a read-only view of the index file, so the
        // kernel reads a dropped page from the file again on its next use:
        // https://man7.org/linux/man-pages/man2/madvise.2.html (MADV_DONTNEED)
        let released = unsafe {
            self.map
                .unchecked_advise_range(UncheckedAdvice::DontNeed, at, len)
        };
        released.ok();
    }

    fn folded_span(&self, RowId(row): RowId) -> Option<(usize, usize)> {
        let start = self.word(self.layout.folded_at, row)?;
        let end = self.word(self.layout.folded_at, row.checked_add(1)?)?;
        Some((usize::try_from(start).ok()?, usize::try_from(end).ok()?))
    }

    fn traits(&self, RowId(row): RowId) -> Option<u8> {
        if row >= self.rows() {
            return None;
        }
        let at = self.layout.traits + usize::try_from(row).ok()?;
        self.map.get(at).copied()
    }

    fn word(&self, column: usize, row: u32) -> Option<u32> {
        if row > self.rows() {
            return None;
        }
        format::u32_at(
            &self.map,
            column + usize::try_from(row).ok()? * size_of::<u32>(),
        )
    }

    pub fn children(&self, folder: RowId) -> impl Iterator<Item = RowId> + '_ {
        let end = self.end(folder).unwrap_or(folder);
        let first = RowId(folder.0.saturating_add(1));
        std::iter::successors(Some(first).filter(|&row| row < end), move |&row| {
            self.end(row).filter(|&next| next < end)
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
            let parent = self.word(self.layout.parents, at.0)?;
            next = (parent != NO_PARENT).then_some(RowId(parent));
            if next.is_some_and(|parent| parent >= at) {
                return None;
            }
        }
        Some(names.iter().rev().collect())
    }

    fn raw(&self, row: RowId) -> Option<Row> {
        let traits = self.traits(row)?;
        let parent = self.word(self.layout.parents, row.0)?;
        let end = self.word(self.layout.ends, row.0)?;
        let at = self.layout.details + usize::try_from(row.0).ok()? * DETAIL_LEN;
        Row::read(parent, end, traits, self.map.get(at..self.layout.names)?)
    }
}

fn check(bytes: &[u8]) -> Result<(Header, Layout), OpenError> {
    let header = Header::read(bytes).ok_or(OpenError::Damaged("shorter than a header"))?;
    if header.magic != MAGIC {
        return Err(OpenError::NotAnIndex);
    }
    if header.version != VERSION {
        return Err(OpenError::OutOfDate);
    }
    if header.rows == 0 || header.rows > u64::from(NO_PARENT) {
        return Err(OpenError::Damaged("row count out of range"));
    }
    let layout = Layout::of(&header).filter(|layout| layout.len == bytes.len());
    let layout = layout.ok_or(OpenError::Damaged("length doesn't match the header"))?;
    check_rows(bytes, &header, &layout)?;
    Ok((header, layout))
}

fn check_rows(bytes: &[u8], header: &Header, layout: &Layout) -> Result<(), OpenError> {
    let column = |at: usize, end: usize| {
        let words = bytes.get(at..end).unwrap_or_default();
        words.as_chunks::<{ size_of::<u32>() }>().0.iter().copied()
    };
    let mut folded_at = column(layout.folded_at, layout.parents).map(u32::from_le_bytes);
    let mut name_start = folded_at.next().unwrap_or_default();
    let parents = column(layout.parents, layout.ends).map(u32::from_le_bytes);
    let ends = column(layout.ends, layout.traits).map(u32::from_le_bytes);
    let traits = bytes.get(layout.traits..layout.folded).unwrap_or_default();
    let mut open: Vec<(u32, u32)> = Vec::new();
    for (((parent, end), &traits), at) in parents.zip(ends).zip(traits).zip(0..) {
        let kind = format::traits_kind(traits);
        if kind.is_none() {
            return Err(OpenError::Damaged("a row of no known kind"));
        }
        if at == 0 && kind != Some(Kind::Folder) {
            return Err(OpenError::Damaged("the first row isn't a folder"));
        }
        let name_end = folded_at.next().unwrap_or_default();
        if name_end < name_start || u64::from(name_end) > header.folded_len {
            return Err(OpenError::Damaged("a name runs past the names block"));
        }
        name_start = name_end;
        while open.last().is_some_and(|&(_, end)| end <= at) {
            open.pop();
        }
        let enclosing = open.last().copied();
        if parent != enclosing.map_or(NO_PARENT, |(folder, _)| folder) {
            return Err(OpenError::Damaged(
                "a row's parent isn't the folder around it",
            ));
        }
        let limit = enclosing.map_or(header.rows, |(_, end)| u64::from(end));
        check_end(kind, (at, end), limit)?;
        if kind == Some(Kind::Folder) {
            open.push((at, end));
        }
    }
    Ok(())
}

fn check_end(kind: Option<Kind>, (at, end): (u32, u32), limit: u64) -> Result<(), OpenError> {
    let end = u64::from(end);
    let next = u64::from(at) + 1;
    let fits = end >= next && end <= limit;
    let leaf_ok = kind == Some(Kind::Folder) || end == next;
    let root_ok = at != 0 || end == limit;
    if fits && leaf_ok && root_ok {
        Ok(())
    } else {
        Err(OpenError::Damaged("a row's run doesn't fit its parent's"))
    }
}

#[cfg(test)]
mod tests;
