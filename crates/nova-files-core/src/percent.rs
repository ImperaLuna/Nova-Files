//! Percent escapes: a byte written as `%` and two hex digits, so any path,
//! UTF-8 or not, fits on one line of text. The job journal and the
//! trash's info files both write paths this way, each keeping its own set
//! of bytes as themselves.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt as _;
use std::path::PathBuf;

const ESCAPE: u8 = b'%';
const HEX: &[u8; 16] = b"0123456789ABCDEF";
const HEX_RADIX: u32 = 16;
/// How far a byte's high hex digit is shifted.
const HIGH: u8 = 4;
const LOW_MASK: u8 = 0x0F;

/// `bytes` with every byte `plain` refuses, and `%` itself, escaped.
pub fn escape(bytes: &[u8], plain: impl Fn(u8) -> bool) -> Vec<u8> {
    let mut escaped = Vec::with_capacity(bytes.len());
    for &byte in bytes {
        if plain(byte) && byte != ESCAPE {
            escaped.push(byte);
        } else {
            escaped.extend([
                ESCAPE,
                HEX[usize::from(byte >> HIGH)],
                HEX[usize::from(byte & LOW_MASK)],
            ]);
        }
    }
    escaped
}

/// The path `field` was escaped from, or `None` when an escape is cut
/// short or isn't hex.
pub fn unescape(field: &[u8]) -> Option<PathBuf> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_escaped_reads_back_byte_for_byte() {
        let bytes = b"/a b/caf\xe9 100%";

        let escaped = escape(bytes, |byte| byte.is_ascii_graphic());

        assert_eq!(escaped, b"/a%20b/caf%E9%20100%25");
        assert_eq!(unescape(&escaped).unwrap().as_os_str().as_bytes(), bytes);
    }

    #[test]
    fn a_cut_short_or_odd_escape_reads_as_nothing() {
        for field in [&b"a%2"[..], b"a%", b"a%zz"] {
            assert_eq!(unescape(field), None, "{field:?}");
        }
        assert_eq!(unescape(b"a%2fb"), Some(PathBuf::from("a/b")));
    }
}
