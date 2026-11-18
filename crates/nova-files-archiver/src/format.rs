//! Finds an archive's format from its first bytes, never from its name: a
//! renamed file still opens, and a file named `.zip` that isn't one is
//! turned away. A tar archive inside gzip, xz, zstd or bzip2 is found by
//! the compression's magic; a plain tar by the `ustar` mark at offset 257
//! (or, for very old archives, by a header whose checksum adds up); an
//! empty tar is only zero blocks. 7z and rar are found by their magic
//! and read through the installed 7-Zip (see `seven`).

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

/// What an archive is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Zip,
    Tar(Compression),
    /// 7z or rar.
    Seven,
}

/// What a tar archive is wrapped in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    Plain,
    Gzip,
    Xz,
    Zstd,
    Bzip2,
}

/// Bytes read to tell the formats apart: one tar header block.
const SNIFF: usize = 512;
const ZIP_ENTRY: &[u8] = b"PK\x03\x04";
/// An empty zip is only its end record.
const ZIP_END: &[u8] = b"PK\x05\x06";
/// A zip split or spanned starts with this mark before its first entry.
const ZIP_SPANNED: &[u8] = b"PK\x07\x08";
const GZIP: &[u8] = b"\x1f\x8b";
const XZ: &[u8] = b"\xfd7zXZ\x00";
const ZSTD: &[u8] = b"\x28\xb5\x2f\xfd";
const BZIP2: &[u8] = b"BZh";
const SEVEN_ZIP: &[u8] = b"7z\xbc\xaf\x27\x1c";
/// Rar 1.5 to 4, and rar 5, whose mark goes on with `\x01\x00`.
const RAR: &[u8] = b"Rar!\x1a\x07";
/// Where a tar header keeps its `ustar` mark, and the mark.
const USTAR_AT: usize = 257;
const USTAR: &[u8] = b"ustar";
/// Where a tar header keeps its checksum, and how long the field is.
const CHECKSUM_AT: usize = 148;
const CHECKSUM_LEN: usize = 8;
/// Tar writes its numbers in octal.
const OCTAL: u64 = 8;

/// The format of the archive `file` holds, if the helper reads it. Leaves
/// `file` at its start.
pub fn sniff(file: &mut File) -> io::Result<Option<Format>> {
    let mut head = Vec::with_capacity(SNIFF);
    file.by_ref().take(SNIFF as u64).read_to_end(&mut head)?;
    file.seek(SeekFrom::Start(0))?;
    Ok(of_head(&head))
}

/// The format these first bytes start.
fn of_head(head: &[u8]) -> Option<Format> {
    let compression = |magic: &[u8], compression| head.starts_with(magic).then_some(compression);
    if [ZIP_ENTRY, ZIP_END, ZIP_SPANNED]
        .iter()
        .any(|magic| head.starts_with(magic))
    {
        return Some(Format::Zip);
    }
    if [SEVEN_ZIP, RAR].iter().any(|magic| head.starts_with(magic)) {
        return Some(Format::Seven);
    }
    compression(GZIP, Compression::Gzip)
        .or_else(|| compression(XZ, Compression::Xz))
        .or_else(|| compression(ZSTD, Compression::Zstd))
        .or_else(|| compression(BZIP2, Compression::Bzip2))
        .or_else(|| is_tar(head).then_some(Compression::Plain))
        .map(Format::Tar)
}

/// Whether `head` is a tar header: the `ustar` mark, a zero block that
/// ends the archive, or for the oldest archives a checksum that matches
/// the block.
fn is_tar(head: &[u8]) -> bool {
    if head.len() < SNIFF {
        return false;
    }
    if head[USTAR_AT..].starts_with(USTAR) || head.iter().all(|&byte| byte == 0) {
        return true;
    }
    let field = &head[CHECKSUM_AT..CHECKSUM_AT + CHECKSUM_LEN];
    let Some(stored) = octal(field) else {
        return false;
    };
    // The checksum counts its own field as spaces.
    let sum: u64 = head
        .iter()
        .enumerate()
        .map(|(at, &byte)| {
            if (CHECKSUM_AT..CHECKSUM_AT + CHECKSUM_LEN).contains(&at) {
                u64::from(b' ')
            } else {
                u64::from(byte)
            }
        })
        .sum();
    sum == stored
}

/// An octal number padded with spaces or NULs, as tar writes them.
fn octal(field: &[u8]) -> Option<u64> {
    let digits = field
        .iter()
        .copied()
        .skip_while(|&byte| byte == b' ')
        .take_while(u8::is_ascii_digit);
    let mut value: u64 = 0;
    let mut any = false;
    for digit in digits {
        let digit = u64::from(digit - b'0');
        if digit >= OCTAL {
            return None;
        }
        value = value.checked_mul(OCTAL)?.checked_add(digit)?;
        any = true;
    }
    any.then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tar_block(mark: bool) -> Vec<u8> {
        let mut block = vec![0_u8; SNIFF];
        block[..5].copy_from_slice(b"a.txt");
        if mark {
            block[USTAR_AT..USTAR_AT + USTAR.len()].copy_from_slice(USTAR);
        }
        block[CHECKSUM_AT..CHECKSUM_AT + CHECKSUM_LEN].fill(b' ');
        let sum: u64 = block.iter().map(|&byte| u64::from(byte)).sum();
        let field = format!("{sum:06o}\0 ");
        block[CHECKSUM_AT..CHECKSUM_AT + CHECKSUM_LEN].copy_from_slice(field.as_bytes());
        block
    }

    #[test]
    fn tells_formats_by_their_first_bytes() {
        assert_eq!(of_head(b"PK\x03\x04rest"), Some(Format::Zip));
        assert_eq!(of_head(b"PK\x05\x06"), Some(Format::Zip));
        assert_eq!(of_head(b"PK\x07\x08PK"), Some(Format::Zip));
        assert_eq!(
            of_head(b"\x1f\x8b\x08"),
            Some(Format::Tar(Compression::Gzip))
        );
        assert_eq!(
            of_head(b"\xfd7zXZ\x00\x00"),
            Some(Format::Tar(Compression::Xz))
        );
        assert_eq!(
            of_head(b"\x28\xb5\x2f\xfd\x00"),
            Some(Format::Tar(Compression::Zstd))
        );
        assert_eq!(of_head(b"BZh91AY"), Some(Format::Tar(Compression::Bzip2)));
        assert_eq!(of_head(b"7z\xbc\xaf\x27\x1c\x00\x04"), Some(Format::Seven));
        assert_eq!(of_head(b"Rar!\x1a\x07\x00"), Some(Format::Seven));
        assert_eq!(of_head(b"Rar!\x1a\x07\x01\x00"), Some(Format::Seven));
        assert_eq!(of_head(b"7z\xbc\xaf\x27"), None);
        assert_eq!(
            of_head(&tar_block(true)),
            Some(Format::Tar(Compression::Plain))
        );
    }

    #[test]
    fn old_tar_is_found_by_its_checksum() {
        assert_eq!(
            of_head(&tar_block(false)),
            Some(Format::Tar(Compression::Plain))
        );
        let mut broken = tar_block(false);
        broken[0] = b'b';
        assert_eq!(of_head(&broken), None);
    }

    #[test]
    fn other_files_are_not_archives() {
        assert_eq!(of_head(b"%PDF-1.7"), None);
        assert_eq!(of_head(b""), None);
        assert_eq!(of_head(&[0; SNIFF - 1]), None);
    }

    #[test]
    fn an_empty_tar_is_zero_blocks() {
        assert_eq!(of_head(&[0; SNIFF]), Some(Format::Tar(Compression::Plain)));
        let mut nearly = [0; SNIFF];
        nearly[SNIFF - 1] = 1;
        assert_eq!(of_head(&nearly), None);
    }

    #[test]
    fn reads_tar_octal_fields() {
        assert_eq!(octal(b"  1750\0 "), Some(0o1750));
        assert_eq!(octal(b"0000644\0"), Some(0o644));
        assert_eq!(octal(b"\0\0\0\0\0\0\0\0"), None);
        assert_eq!(octal(b"  78\0"), None);
    }
}
