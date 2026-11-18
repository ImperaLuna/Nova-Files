use crate::{FileId, Kind, RowId, Stamp};

pub const MAGIC: [u8; MAGIC_LEN] = *b"NOVAIDX\0";
const MAGIC_LEN: usize = 8;
pub const VERSION: u32 = 1;
pub const HEADER_LEN: usize = 48;
pub const ROW_LEN: usize = 56;
pub const NO_PARENT: u32 = u32::MAX;
pub const NO_STAMP: i64 = i64::MIN;

const H_MAGIC: usize = 0;
const H_VERSION: usize = 8;
const H_ROW_LEN: usize = 12;
const H_ROWS: usize = 16;
const H_NAMES_LEN: usize = 24;
const H_CRAWLED: usize = 32;

const R_PARENT: usize = 0;
const R_END: usize = 4;
const R_NAME_AT: usize = 8;
const R_NAME_LEN: usize = 12;
const R_KIND: usize = 14;
const R_FLAGS: usize = 15;
const R_DEVICE: usize = 16;
const R_INODE: usize = 24;
const R_SIZE: usize = 32;
const R_MODIFIED: usize = 40;
const R_CREATED: usize = 48;

const KIND_FOLDER: u8 = 0;
const KIND_FILE: u8 = 1;
const KIND_LINK: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub magic: [u8; MAGIC_LEN],
    pub version: u32,
    pub row_len: u32,
    pub rows: u64,
    pub names_len: u64,
    pub crawled: i64,
}

impl Header {
    pub const fn current(rows: u64, names_len: u64, crawled: i64) -> Self {
        Self {
            magic: MAGIC,
            version: VERSION,
            #[expect(
                clippy::cast_possible_truncation,
                reason = "ROW_LEN is a small constant"
            )]
            row_len: ROW_LEN as u32,
            rows,
            names_len,
            crawled,
        }
    }

    pub fn read(bytes: &[u8]) -> Option<Self> {
        let bytes = bytes.get(..HEADER_LEN)?;
        Some(Self {
            magic: bytes.get(H_MAGIC..H_VERSION)?.try_into().ok()?,
            version: u32_at(bytes, H_VERSION)?,
            row_len: u32_at(bytes, H_ROW_LEN)?,
            rows: u64_at(bytes, H_ROWS)?,
            names_len: u64_at(bytes, H_NAMES_LEN)?,
            crawled: i64_at(bytes, H_CRAWLED)?,
        })
    }

    pub fn write(&self) -> [u8; HEADER_LEN] {
        let mut out = [0; HEADER_LEN];
        put(&mut out, H_MAGIC, &self.magic);
        put(&mut out, H_VERSION, &self.version.to_le_bytes());
        put(&mut out, H_ROW_LEN, &self.row_len.to_le_bytes());
        put(&mut out, H_ROWS, &self.rows.to_le_bytes());
        put(&mut out, H_NAMES_LEN, &self.names_len.to_le_bytes());
        put(&mut out, H_CRAWLED, &self.crawled.to_le_bytes());
        out
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
    pub fn read(bytes: &[u8]) -> Option<Self> {
        let bytes = bytes.get(..ROW_LEN)?;
        Some(Self {
            parent: u32_at(bytes, R_PARENT)?,
            end: u32_at(bytes, R_END)?,
            name_at: u32_at(bytes, R_NAME_AT)?,
            name_len: u16::from_le_bytes(bytes.get(R_NAME_LEN..R_KIND)?.try_into().ok()?),
            kind: kind_from(*bytes.get(R_KIND)?),
            flags: *bytes.get(R_FLAGS)?,
            id: FileId {
                device: u64_at(bytes, R_DEVICE)?,
                inode: u64_at(bytes, R_INODE)?,
            },
            size: u64_at(bytes, R_SIZE)?,
            modified: i64_at(bytes, R_MODIFIED)?,
            created: i64_at(bytes, R_CREATED)?,
        })
    }

    pub fn write(&self) -> [u8; ROW_LEN] {
        let mut out = [0; ROW_LEN];
        put(&mut out, R_PARENT, &self.parent.to_le_bytes());
        put(&mut out, R_END, &self.end.to_le_bytes());
        put(&mut out, R_NAME_AT, &self.name_at.to_le_bytes());
        put(&mut out, R_NAME_LEN, &self.name_len.to_le_bytes());
        put(&mut out, R_KIND, &[self.kind.map_or(u8::MAX, kind_byte)]);
        put(&mut out, R_FLAGS, &[self.flags]);
        put(&mut out, R_DEVICE, &self.id.device.to_le_bytes());
        put(&mut out, R_INODE, &self.id.inode.to_le_bytes());
        put(&mut out, R_SIZE, &self.size.to_le_bytes());
        put(&mut out, R_MODIFIED, &self.modified.to_le_bytes());
        put(&mut out, R_CREATED, &self.created.to_le_bytes());
        out
    }

    pub fn parent(&self) -> Option<RowId> {
        (self.parent != NO_PARENT).then_some(RowId(self.parent))
    }
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
        let header = Header::current(7, 99, -5);
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
            assert_eq!(Row::read(&row.write()), Some(row));
            assert_eq!(Row::read(&row.write()[..ROW_LEN - 1]), None);
        }
    }

    #[test]
    fn only_row_zero_has_no_parent() {
        let mut row = Row::read(&[0; ROW_LEN]).unwrap();
        assert_eq!(row.parent(), Some(RowId(0)));
        row.parent = NO_PARENT;
        assert_eq!(row.parent(), None);
    }

    #[test]
    fn a_missing_time_round_trips() {
        assert_eq!(stamp(stored(None)), None);
        assert_eq!(stamp(stored(Some(Stamp(12)))), Some(Stamp(12)));
    }
}
