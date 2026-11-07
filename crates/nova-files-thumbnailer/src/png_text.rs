//! Adds `tEXt` chunks to an encoded PNG, which is how the freedesktop
//! thumbnail spec stores a thumbnail's source URI and mtime.
//!
//! The chunks go right after `IHDR`, which the PNG format requires to come
//! first. Each chunk carries a CRC-32 of its type and data.

/// The PNG signature, then `IHDR`: length, type, 13 bytes of data, CRC.
const IHDR_END: usize = 8 + 4 + 4 + 13 + 4;
const TEXT_TYPE: &[u8; 4] = b"tEXt";
/// The reflected CRC-32 polynomial PNG uses.
const CRC_POLYNOMIAL: u32 = 0xEDB8_8320;
const BITS_PER_BYTE: u32 = 8;

/// `png` with a `tEXt` chunk for each `(keyword, text)`, or `None` when
/// `png` is too short to hold an `IHDR`.
pub fn with_text(png: &[u8], entries: &[(&str, &str)]) -> Option<Vec<u8>> {
    let (head, tail) = png.split_at_checked(IHDR_END)?;
    let mut out = Vec::with_capacity(png.len());
    out.extend_from_slice(head);
    for (keyword, text) in entries {
        let mut data = Vec::with_capacity(keyword.len() + 1 + text.len());
        data.extend_from_slice(keyword.as_bytes());
        data.push(0);
        data.extend_from_slice(text.as_bytes());
        let length = u32::try_from(data.len()).ok()?;
        let mut checked = Vec::with_capacity(TEXT_TYPE.len() + data.len());
        checked.extend_from_slice(TEXT_TYPE);
        checked.extend_from_slice(&data);
        out.extend_from_slice(&length.to_be_bytes());
        out.extend_from_slice(&checked);
        out.extend_from_slice(&crc32(&checked).to_be_bytes());
    }
    out.extend_from_slice(tail);
    Some(out)
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..BITS_PER_BYTE {
            let low_bit_set = crc & 1 == 1;
            crc >>= 1;
            if low_bit_set {
                crc ^= CRC_POLYNOMIAL;
            }
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn text_goes_right_after_the_header() {
        let png = [vec![0; IHDR_END], b"rest".to_vec()].concat();
        let out = with_text(&png, &[("Thumb::MTime", "7")]).unwrap();
        let chunk = &out[IHDR_END..out.len() - b"rest".len()];
        assert_eq!(&chunk[..4], &14_u32.to_be_bytes());
        assert_eq!(&chunk[4..8], b"tEXt");
        assert_eq!(&chunk[8..22], b"Thumb::MTime\x007");
        assert_eq!(&chunk[22..], &crc32(b"tEXtThumb::MTime\x007").to_be_bytes());
        assert!(out.ends_with(b"rest"));
    }

    #[test]
    fn too_short_for_a_header() {
        assert_eq!(with_text(&[0; IHDR_END - 1], &[]), None);
    }
}
