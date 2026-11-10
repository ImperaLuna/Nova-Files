//! Info files: `info/<name>.trashinfo`, which says where the entry
//! `files/<name>` came from and when it was trashed.
//!
//! ```text
//! [Trash Info]
//! Path=/home/me/My%20Photos
//! DeletionDate=2026-11-10T04:38:19
//! ```
//!
//! The path is written byte by byte, every byte but letters, digits,
//! `-._~` and `/` as `%` and two hex digits, so any name, UTF-8 or not,
//! fits. The date is local time, as the spec says and other apps read
//! it.
//!
//! An entry keeps its own name in the trash when it is free, else the
//! first free of `name.2`, `name.3` and so on, cut short so the info
//! file's name still fits, between characters when the name is UTF-8.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::Path;
use std::time::SystemTime;

use chrono::{DateTime, Local};

use crate::percent;

/// The end of every info file's name.
pub const INFO_SUFFIX: &str = ".trashinfo";
const HEADER: &str = "[Trash Info]";
const PATH_KEY: &str = "Path=";
const DATE_KEY: &str = "DeletionDate=";
/// The longest name most filesystems take, in bytes.
const MAX_NAME: usize = 255;
/// Bytes a path keeps as themselves, besides letters and digits.
const PLAIN_MARKS: &[u8] = b"-._~/";

/// How `DeletionDate` is written: local time, without a zone.
const DATE_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";

/// The info file's text for an entry from `path`, as the trash writes
/// paths (see `TrashFolder::path_field`), trashed at `when`.
pub fn info_text(path: &Path, when: SystemTime) -> Vec<u8> {
    let mut text = Vec::new();
    text.extend_from_slice(HEADER.as_bytes());
    text.push(b'\n');
    text.extend_from_slice(PATH_KEY.as_bytes());
    text.extend(escape(path.as_os_str().as_bytes()));
    text.push(b'\n');
    text.extend_from_slice(DATE_KEY.as_bytes());
    text.extend_from_slice(date(when).as_bytes());
    text.push(b'\n');
    text
}

/// `bytes` with every byte but letters, digits and [`PLAIN_MARKS`]
/// escaped.
pub(super) fn escape(bytes: &[u8]) -> Vec<u8> {
    percent::escape(bytes, |byte| {
        byte.is_ascii_alphanumeric() || PLAIN_MARKS.contains(&byte)
    })
}

/// `when` as `YYYY-MM-DDThh:mm:ss` in local time. Without the system's
/// time zone, chrono takes UTC.
fn date(when: SystemTime) -> String {
    DateTime::<Local>::from(when)
        .format(DATE_FORMAT)
        .to_string()
}

/// The `attempt`th name an entry named `name` tries in the trash: its
/// own first, then `name.2` and on, cut short so that the name with
/// [`INFO_SUFFIX`] fits in a folder.
pub fn stored_name(name: &OsStr, attempt: u32) -> OsString {
    let number = if attempt <= 1 {
        String::new()
    } else {
        format!(".{attempt}")
    };
    let mut room = MAX_NAME - INFO_SUFFIX.len() - number.len();
    // A name in UTF-8 is cut between characters, so it stays UTF-8.
    if let Ok(text) = str::from_utf8(name.as_bytes()) {
        room = text.floor_char_boundary(room);
    }
    let mut bytes = name.as_bytes().to_vec();
    bytes.truncate(room);
    bytes.extend_from_slice(number.as_bytes());
    OsString::from_vec(bytes)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use chrono::{NaiveDateTime, TimeZone as _};

    use super::*;

    #[test]
    fn writes_the_path_escaped_and_the_date() {
        let when = UNIX_EPOCH + Duration::from_secs(1_794_285_499);
        let path = Path::new(OsStr::from_bytes(b"/home/me/My Photos/caf\xe9 100%.txt"));

        let text = info_text(path, when);

        assert_eq!(
            String::from_utf8(text).unwrap(),
            format!(
                "[Trash Info]\nPath=/home/me/My%20Photos/caf%E9%20100%25.txt\n\
                 DeletionDate={}\n",
                date(when)
            )
        );
    }

    #[test]
    fn the_date_is_local_time_to_the_second() {
        let when = UNIX_EPOCH + Duration::from_secs(1_794_285_499);

        let written = date(when);

        let read = NaiveDateTime::parse_from_str(&written, DATE_FORMAT).unwrap();
        let back = Local.from_local_datetime(&read).earliest().unwrap();
        assert_eq!(SystemTime::from(back), when);
        assert_eq!(written.len(), "2026-11-10T04:38:19".len());
    }

    #[test]
    fn names_take_a_number_after_the_first_try() {
        let name = OsStr::new("notes.txt");
        assert_eq!(stored_name(name, 0), "notes.txt");
        assert_eq!(stored_name(name, 1), "notes.txt");
        assert_eq!(stored_name(name, 2), "notes.txt.2");
        assert_eq!(stored_name(name, 13), "notes.txt.13");
    }

    #[test]
    fn a_long_name_is_cut_so_its_info_file_fits() {
        let name = OsString::from_vec(vec![b'a'; MAX_NAME]);

        for attempt in [1, 2, 100] {
            let stored = stored_name(&name, attempt);
            assert_eq!(stored.len() + INFO_SUFFIX.len(), MAX_NAME, "{attempt}");
        }
        assert!(stored_name(&name, 100).as_bytes().ends_with(b".100"));
    }

    #[test]
    fn a_long_utf8_name_is_cut_between_characters() {
        // 83 characters of 3 bytes: 249 bytes, cut inside the 82nd.
        let name = OsString::from("日".repeat(83));

        let stored = stored_name(&name, 1);

        assert_eq!(stored, OsString::from("日".repeat(81)));
    }
}
