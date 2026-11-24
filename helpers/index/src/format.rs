use std::io::{self, Read as _, Write};

use crate::fold;
use crate::{FileId, Kind, RowId, Stamp};

pub const MAGIC: [u8; MAGIC_LEN] = *b"NOVAIDX\0";
const MAGIC_LEN: usize = 8;
pub const VERSION: u32 = 4;
pub const HEADER_LEN: usize = 48;
pub const DETAIL_LEN: usize = 48;
pub const NO_PARENT: u32 = u32::MAX;
pub const NO_STAMP: i64 = i64::MIN;
const PAGE: usize = 4096;
const WORD: usize = size_of::<u32>();
const BYTE_BITS: usize = 8;

const H_MAGIC: usize = 0;
const H_VERSION: usize = 8;
const H_ROWS: usize = 16;
const H_NAMES_LEN: usize = 24;
const H_FOLDED_LEN: usize = 32;
const H_CRAWLED: usize = 40;

const D_NAME_AT: usize = 0;
const D_NAME_LEN: usize = 4;
const D_DEVICE: usize = 8;
const D_INODE: usize = 16;
const D_SIZE: usize = 24;
const D_MODIFIED: usize = 32;
const D_CREATED: usize = 40;

const KIND_FOLDER: u8 = 0;
const KIND_FILE: u8 = 1;
const KIND_LINK: u8 = 2;
const KIND_BITS: u8 = 0b11;
const HIDDEN_BIT: u8 = 1 << 2;
const AS_FOLDED_BIT: u8 = 1 << 3;
const FLAGS_SHIFT: u32 = 4;
const FLAGS_BITS: u8 = 0b1111;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub magic: [u8; MAGIC_LEN],
    pub version: u32,
    pub rows: u64,
    pub names_len: u64,
    pub folded_len: u64,
    pub crawled: i64,
}

impl Header {
    pub const fn current(rows: u64, names_len: u64, folded_len: u64, crawled: i64) -> Self {
        Self {
            magic: MAGIC,
            version: VERSION,
            rows,
            names_len,
            folded_len,
            crawled,
        }
    }

    pub fn read(bytes: &[u8]) -> Option<Self> {
        let bytes = bytes.get(..HEADER_LEN)?;
        Some(Self {
            magic: bytes.get(H_MAGIC..H_VERSION)?.try_into().ok()?,
            version: u32_at(bytes, H_VERSION)?,
            rows: u64_at(bytes, H_ROWS)?,
            names_len: u64_at(bytes, H_NAMES_LEN)?,
            folded_len: u64_at(bytes, H_FOLDED_LEN)?,
            crawled: i64_at(bytes, H_CRAWLED)?,
        })
    }

    pub fn write(&self) -> [u8; HEADER_LEN] {
        let mut out = [0; HEADER_LEN];
        put(&mut out, H_MAGIC, &self.magic);
        put(&mut out, H_VERSION, &self.version.to_le_bytes());
        put(&mut out, H_ROWS, &self.rows.to_le_bytes());
        put(&mut out, H_NAMES_LEN, &self.names_len.to_le_bytes());
        put(&mut out, H_FOLDED_LEN, &self.folded_len.to_le_bytes());
        put(&mut out, H_CRAWLED, &self.crawled.to_le_bytes());
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub folded_at: usize,
    pub parents: usize,
    pub ends: usize,
    pub letters: usize,
    pub traits: usize,
    pub folded: usize,
    pub word_starts: usize,
    pub details: usize,
    pub names: usize,
    pub len: usize,
}

impl Layout {
    pub fn of(header: &Header) -> Option<Self> {
        let rows = usize::try_from(header.rows).ok()?;
        let names_len = usize::try_from(header.names_len).ok()?;
        let folded_len = usize::try_from(header.folded_len).ok()?;
        let folded_at = PAGE;
        let parents = rows
            .checked_add(1)?
            .checked_mul(WORD)?
            .checked_add(folded_at)?;
        let ends = rows.checked_mul(WORD)?.checked_add(parents)?;
        let letters = rows.checked_mul(WORD)?.checked_add(ends)?;
        let traits = rows.checked_mul(WORD)?.checked_add(letters)?;
        let folded = traits.checked_add(rows)?;
        let word_starts = folded.checked_add(folded_len)?;
        let details = word_starts
            .checked_add(folded_len.div_ceil(BYTE_BITS))?
            .checked_next_multiple_of(PAGE)?;
        let names = rows.checked_mul(DETAIL_LEN)?.checked_add(details)?;
        let len = names.checked_add(names_len)?;
        Some(Self {
            folded_at,
            parents,
            ends,
            letters,
            traits,
            folded,
            word_starts,
            details,
            names,
            len,
        })
    }
}

pub const fn traits(kind: Option<Kind>, hidden: bool, flags: u8) -> u8 {
    let kind = match kind {
        Some(kind) => kind_byte(kind),
        None => KIND_BITS,
    };
    let hidden = if hidden { HIDDEN_BIT } else { 0 };
    kind | hidden | ((flags & FLAGS_BITS) << FLAGS_SHIFT)
}

pub const fn traits_kind(traits: u8) -> Option<Kind> {
    kind_from(traits & KIND_BITS)
}

pub const fn traits_hidden(traits: u8) -> bool {
    traits & HIDDEN_BIT != 0
}

pub const fn traits_as_folded(traits: u8) -> bool {
    traits & AS_FOLDED_BIT != 0
}

pub const fn traits_flags(traits: u8) -> u8 {
    traits >> FLAGS_SHIFT
}

pub fn bit_at(bits: &[u8], at: usize) -> bool {
    bits.get(at / BYTE_BITS)
        .is_some_and(|byte| byte & (1 << (at % BYTE_BITS)) != 0)
}

fn set_bit(bits: &mut [u8], at: usize) {
    if let Some(byte) = bits.get_mut(at / BYTE_BITS) {
        *byte |= 1 << (at % BYTE_BITS);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub parent: u32,
    pub end: u32,
    pub name_at: u32,
    pub name_len: u16,
    pub kind: Option<Kind>,
    pub flags: u8,
    pub id: FileId,
    pub size: u64,
    pub modified: i64,
    pub created: i64,
}

impl Row {
    pub fn detail(&self) -> [u8; DETAIL_LEN] {
        let mut out = [0; DETAIL_LEN];
        put(&mut out, D_NAME_AT, &self.name_at.to_le_bytes());
        put(&mut out, D_NAME_LEN, &self.name_len.to_le_bytes());
        put(&mut out, D_DEVICE, &self.id.device.to_le_bytes());
        put(&mut out, D_INODE, &self.id.inode.to_le_bytes());
        put(&mut out, D_SIZE, &self.size.to_le_bytes());
        put(&mut out, D_MODIFIED, &self.modified.to_le_bytes());
        put(&mut out, D_CREATED, &self.created.to_le_bytes());
        out
    }

    pub fn read(parent: u32, end: u32, traits: u8, detail: &[u8]) -> Option<Self> {
        let detail = detail.get(..DETAIL_LEN)?;
        Some(Self {
            parent,
            end,
            name_at: u32_at(detail, D_NAME_AT)?,
            name_len: u16_at(detail, D_NAME_LEN)?,
            kind: traits_kind(traits),
            flags: traits_flags(traits),
            id: FileId {
                device: u64_at(detail, D_DEVICE)?,
                inode: u64_at(detail, D_INODE)?,
            },
            size: u64_at(detail, D_SIZE)?,
            modified: i64_at(detail, D_MODIFIED)?,
            created: i64_at(detail, D_CREATED)?,
        })
    }

    pub fn parent(&self) -> Option<RowId> {
        (self.parent != NO_PARENT).then_some(RowId(self.parent))
    }

    fn name<'a>(&self, names: &'a [u8]) -> &'a [u8] {
        let at = usize::try_from(self.name_at).unwrap_or(usize::MAX);
        names
            .get(at..at.saturating_add(usize::from(self.name_len)))
            .unwrap_or_default()
    }
}

pub fn write(out: &mut impl Write, crawled: i64, rows: &[Row], names: &[u8]) -> io::Result<()> {
    let mut folded = Vec::with_capacity(names.len());
    let mut folded_at = Vec::with_capacity(rows.len() + 1);
    let mut word_starts = Vec::new();
    let mut traits_of: Vec<u8> = Vec::with_capacity(rows.len());
    let mut letters = Vec::with_capacity(rows.len());
    let mut one = Vec::new();
    for (at, row) in rows.iter().enumerate() {
        let name = row.name(names);
        folded_at.push(u32::try_from(folded.len()).map_err(io::Error::other)?);
        fold::fold_into(name, &mut one);
        word_starts.resize((folded.len() + one.len()).div_ceil(BYTE_BITS), 0);
        for start in (0..one.len()).filter(|&start| fold::starts_word(name, &one, start)) {
            set_bit(&mut word_starts, folded.len() + start);
        }
        letters.push(fold::letters(&one));
        folded.extend_from_slice(&one);
        let under_hidden = usize::try_from(row.parent)
            .ok()
            .and_then(|parent| traits_of.get(parent))
            .is_some_and(|&parent| traits_hidden(parent));
        let hidden = at != 0 && (under_hidden || name.first() == Some(&b'.'));
        let as_folded = if !name.is_empty() && name == one.as_slice() {
            AS_FOLDED_BIT
        } else {
            0
        };
        traits_of.push(traits(row.kind, hidden, row.flags) | as_folded);
    }
    folded_at.push(u32::try_from(folded.len()).map_err(io::Error::other)?);
    let header = Header::current(
        u64::try_from(rows.len()).map_err(io::Error::other)?,
        u64::try_from(names.len()).map_err(io::Error::other)?,
        u64::try_from(folded.len()).map_err(io::Error::other)?,
        crawled,
    );
    let layout = Layout::of(&header).ok_or_else(|| io::Error::other("the index is too big"))?;
    out.write_all(&header.write())?;
    pad(out, layout.folded_at - HEADER_LEN)?;
    for at in &folded_at {
        out.write_all(&at.to_le_bytes())?;
    }
    for row in rows {
        out.write_all(&row.parent.to_le_bytes())?;
    }
    for row in rows {
        out.write_all(&row.end.to_le_bytes())?;
    }
    for mask in &letters {
        out.write_all(&mask.to_le_bytes())?;
    }
    out.write_all(&traits_of)?;
    out.write_all(&folded)?;
    out.write_all(&word_starts)?;
    pad(out, layout.details - layout.word_starts - word_starts.len())?;
    for row in rows {
        out.write_all(&row.detail())?;
    }
    out.write_all(names)
}

fn pad(out: &mut impl Write, len: usize) -> io::Result<()> {
    io::copy(&mut io::repeat(0).take(len as u64), out).map(drop)
}

pub const fn stamp(stored: i64) -> Option<Stamp> {
    if stored == NO_STAMP {
        None
    } else {
        Some(Stamp(stored))
    }
}

pub const fn stored(stamp: Option<Stamp>) -> i64 {
    match stamp {
        Some(Stamp(nanos)) => nanos,
        None => NO_STAMP,
    }
}

pub const fn kind_byte(kind: Kind) -> u8 {
    match kind {
        Kind::Folder => KIND_FOLDER,
        Kind::File => KIND_FILE,
        Kind::Link => KIND_LINK,
    }
}

pub const fn kind_from(byte: u8) -> Option<Kind> {
    match byte {
        KIND_FOLDER => Some(Kind::Folder),
        KIND_FILE => Some(Kind::File),
        KIND_LINK => Some(Kind::Link),
        _ => None,
    }
}

pub fn put(out: &mut [u8], at: usize, bytes: &[u8]) {
    if let Some(slot) = out.get_mut(at..at + bytes.len()) {
        slot.copy_from_slice(bytes);
    }
}

pub fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(at..at + size_of::<u16>())?.try_into().ok()?,
    ))
}

pub fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(at..at + size_of::<u32>())?.try_into().ok()?,
    ))
}

pub fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(at..at + size_of::<u64>())?.try_into().ok()?,
    ))
}

pub fn i64_at(bytes: &[u8], at: usize) -> Option<i64> {
    Some(i64::from_le_bytes(
        bytes.get(at..at + size_of::<i64>())?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_reads_back_what_was_written() {
        let header = Header::current(7, 99, 101, -5);
        assert_eq!(Header::read(&header.write()), Some(header));
        assert_eq!(Header::read(&header.write()[..HEADER_LEN - 1]), None);
    }

    #[test]
    fn a_row_reads_back_what_was_written() {
        for kind in [Some(Kind::Folder), Some(Kind::File), Some(Kind::Link), None] {
            let row = Row {
                parent: 3,
                end: 9,
                name_at: 40,
                name_len: 6,
                kind,
                flags: 2,
                id: FileId {
                    device: 66,
                    inode: 1 << 40,
                },
                size: 1 << 33,
                modified: -7,
                created: NO_STAMP,
            };
            let traits = traits(row.kind, true, row.flags);
            assert!(traits_hidden(traits));
            assert_eq!(Row::read(3, 9, traits, &row.detail()), Some(row));
            assert_eq!(
                Row::read(3, 9, traits, &row.detail()[..DETAIL_LEN - 1]),
                None
            );
        }
    }

    #[test]
    fn the_parts_follow_each_other_and_the_details_start_a_page() {
        let layout = Layout::of(&Header::current(3, 10, 9, NO_STAMP)).unwrap();
        assert_eq!(layout.folded_at, PAGE);
        assert_eq!(layout.parents, PAGE + 4 * WORD);
        assert_eq!(layout.ends, layout.parents + 3 * WORD);
        assert_eq!(layout.letters, layout.ends + 3 * WORD);
        assert_eq!(layout.traits, layout.letters + 3 * WORD);
        assert_eq!(layout.folded, layout.traits + 3);
        assert_eq!(layout.word_starts, layout.folded + 9);
        assert_eq!(layout.details, 2 * PAGE);
        assert_eq!(layout.names, layout.details + 3 * DETAIL_LEN);
        assert_eq!(layout.len, layout.names + 10);
        assert_eq!(Layout::of(&Header::current(u64::MAX, 0, 0, NO_STAMP)), None);
    }

    #[test]
    fn a_missing_time_round_trips() {
        assert_eq!(stamp(stored(None)), None);
        assert_eq!(stamp(stored(Some(Stamp(12)))), Some(Stamp(12)));
    }
}
