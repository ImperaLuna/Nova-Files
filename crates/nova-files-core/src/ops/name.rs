//! Whether a name a user typed can be one entry's name.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt as _;

/// The byte that ends a C string; no name may hold it.
const NUL: u8 = b'\0';
const SEPARATOR: u8 = b'/';

/// `true` for a name that names one entry inside a folder: not empty, not
/// `.` or `..`, and without a `/` or a NUL byte.
pub fn is_plain_name(name: &OsStr) -> bool {
    let bytes = name.as_bytes();
    !matches!(bytes, b"" | b"." | b"..") && !bytes.contains(&SEPARATOR) && !bytes.contains(&NUL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_names_of_one_entry() {
        assert!(is_plain_name(OsStr::new("notes.txt")));
        assert!(is_plain_name(OsStr::new(".hidden")));
        assert!(is_plain_name(OsStr::new("...")));
        assert!(is_plain_name(OsStr::from_bytes(b"caf\xe9")));
    }

    #[test]
    fn refuses_names_that_leave_the_folder_or_name_none() {
        for name in ["", ".", "..", "a/b", "/", "../x"] {
            assert!(!is_plain_name(OsStr::new(name)), "{name:?}");
        }
        assert!(!is_plain_name(OsStr::from_bytes(b"a\0b")));
    }
}
