//! `file://` URIs, as D-Bus `Open` calls and desktop launchers pass them.
//!
//! A path is bytes, not text, so both directions work on bytes: every byte
//! outside the URI's unreserved set is percent-encoded, and decoding gives
//! back the exact bytes, valid UTF-8 or not.

use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Path, PathBuf};

const SCHEME: &str = "file://";
/// The one host a local file URI may name besides the empty one.
const LOCALHOST: &str = "localhost";
const HEX_RADIX: u32 = 16;
/// Digits after each `%`.
const HEX_LENGTH: usize = 2;
const HEX_DIGITS: &[u8; 16] = b"0123456789ABCDEF";
const HIGH_NIBBLE: u32 = 4;
const LOW_NIBBLE_MASK: u8 = 0x0f;

/// The local path a `file://` URI names, or `None` for any other URI, a
/// remote host, a relative path or broken percent-encoding.
pub fn path_from_file_uri(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix(SCHEME)?;
    let path = match rest.find('/') {
        Some(0) => rest,
        Some(slash) if &rest[..slash] == LOCALHOST => &rest[slash..],
        _ => return None,
    };
    // A query or fragment has no meaning for a local folder.
    if path.contains(['?', '#']) {
        return None;
    }
    let bytes = percent_decode(path.as_bytes())?;
    Some(PathBuf::from(OsString::from_vec(bytes)))
}

/// The `file://` URI for an absolute `path`.
pub fn file_uri(path: &Path) -> String {
    let mut uri = String::from(SCHEME);
    for &byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            uri.push(char::from(byte));
        } else {
            uri.push('%');
            uri.push(char::from(HEX_DIGITS[usize::from(byte >> HIGH_NIBBLE)]));
            uri.push(char::from(HEX_DIGITS[usize::from(byte & LOW_NIBBLE_MASK)]));
        }
    }
    uri
}

fn percent_decode(text: &[u8]) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text;
    while let Some((&byte, after)) = rest.split_first() {
        if byte == b'%' {
            let (hex, after) = after.split_at_checked(HEX_LENGTH)?;
            let hex = std::str::from_utf8(hex).ok()?;
            // from_str_radix would also take a sign.
            if !hex.bytes().all(|digit| digit.is_ascii_hexdigit()) {
                return None;
            }
            bytes.push(u8::from_str_radix(hex, HEX_RADIX).ok()?);
            rest = after;
        } else {
            bytes.push(byte);
            rest = after;
        }
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_local_uri() {
        assert_eq!(
            path_from_file_uri("file:///home/me/My%20Files"),
            Some(PathBuf::from("/home/me/My Files"))
        );
        assert_eq!(
            path_from_file_uri("file://localhost/tmp"),
            Some(PathBuf::from("/tmp"))
        );
    }

    #[test]
    fn refuses_what_is_not_a_local_path() {
        assert_eq!(path_from_file_uri("https://example.com/x"), None);
        assert_eq!(path_from_file_uri("file://server/share"), None);
        assert_eq!(path_from_file_uri("file:///bad%2"), None);
        assert_eq!(path_from_file_uri("file:///bad%zz"), None);
        assert_eq!(path_from_file_uri("file:///bad%+1"), None);
        assert_eq!(path_from_file_uri("file:///a?b"), None);
    }

    #[test]
    fn round_trips_any_bytes() {
        let path = PathBuf::from(OsString::from_vec(b"/tmp/a b%\xff\xc3\xa9".to_vec()));
        let uri = file_uri(&path);
        assert_eq!(uri, "file:///tmp/a%20b%25%FF%C3%A9");
        assert_eq!(path_from_file_uri(&uri), Some(path));
    }
}
