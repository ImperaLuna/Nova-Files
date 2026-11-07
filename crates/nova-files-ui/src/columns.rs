//! What the Modified, Type and Size columns show for one entry: plain text
//! worked out from the entry and its details, no disk reads.
//!
//! Sizes use decimal units (1 KB is 1,000 bytes), one decimal below 10.
//! Dates are local time. The type comes from the name's extension alone;
//! a name without one is a plain "File".

use std::ffi::OsStr;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local};
use nova_files_api::EntryKind;

/// The text of an entry's Modified, Type and Size columns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColumnText {
    pub modified: String,
    pub type_name: String,
    pub size: String,
}

/// Decimal size units after bytes, each 1,000 times the one before.
const SIZE_UNITS: [&str; 5] = ["KB", "MB", "GB", "TB", "PB"];
const SIZE_STEP: f64 = 1000.0;
/// Below this many units a size shows one decimal ("1.8 MB").
const ONE_DECIMAL_BELOW: f64 = 10.0;
const DATE_FORMAT: &str = "%Y-%m-%d %H:%M";

const FOLDER_TYPE: &str = "Folder";
const PLAIN_FILE_TYPE: &str = "File";

/// Known extensions, lower case, and their type names.
const KNOWN_TYPES: &[(&str, &str)] = &[
    ("jpg", "JPEG image"),
    ("jpeg", "JPEG image"),
    ("png", "PNG image"),
    ("webp", "WebP image"),
    ("gif", "GIF image"),
    ("heic", "HEIC image"),
    ("svg", "SVG image"),
    ("mp4", "MP4 video"),
    ("mkv", "MKV video"),
    ("webm", "WebM video"),
    ("mov", "QuickTime video"),
    ("mp3", "MP3 audio"),
    ("flac", "FLAC audio"),
    ("ogg", "Ogg audio"),
    ("txt", "Text"),
    ("md", "Markdown"),
    ("pdf", "PDF document"),
    ("odt", "Text document"),
    ("ods", "Spreadsheet"),
    ("zip", "ZIP archive"),
    ("tar", "Tar archive"),
    ("gz", "Gzip archive"),
    ("rs", "Rust source"),
];

/// "512 B", "1.8 MB", "640 KB".
pub fn size(bytes: u64) -> String {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a shown size keeps at most two significant digits"
    )]
    let mut value = bytes as f64;
    if value < SIZE_STEP {
        return format!("{bytes} B");
    }
    let mut unit = SIZE_UNITS[0];
    for next in SIZE_UNITS {
        value /= SIZE_STEP;
        unit = next;
        if value < SIZE_STEP {
            break;
        }
    }
    if value < ONE_DECIMAL_BELOW {
        format!("{value:.1} {unit}")
    } else {
        format!("{value:.0} {unit}")
    }
}

/// The time in local time, to the minute; empty for a time too far from
/// now to show.
pub fn modified(time: SystemTime) -> String {
    let since_epoch = match time.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).ok(),
        Err(before) => {
            let before = before.duration();
            // Round away from the epoch, so 0.5 s before it is 1 s before.
            let secs = before.as_secs() + u64::from(before.subsec_nanos() > 0);
            i64::try_from(secs).ok().map(|secs| -secs)
        }
    };
    since_epoch
        .and_then(|secs| DateTime::from_timestamp(secs, 0))
        .map(|time| time.with_timezone(&Local).format(DATE_FORMAT).to_string())
        .unwrap_or_default()
}

/// The type name for an entry called `name`.
pub fn type_name(name: &OsStr, kind: EntryKind) -> String {
    if kind == EntryKind::Folder {
        return FOLDER_TYPE.to_owned();
    }
    let Some(extension) = Path::new(name).extension() else {
        return PLAIN_FILE_TYPE.to_owned();
    };
    let extension = extension.to_string_lossy().to_lowercase();
    KNOWN_TYPES
        .iter()
        .find(|(known, _)| *known == extension)
        .map_or_else(
            || format!("{} file", extension.to_uppercase()),
            |(_, name)| (*name).to_owned(),
        )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn sizes_use_decimal_units() {
        assert_eq!(size(0), "0 B");
        assert_eq!(size(999), "999 B");
        assert_eq!(size(1000), "1.0 KB");
        assert_eq!(size(640_000), "640 KB");
        assert_eq!(size(1_800_000), "1.8 MB");
        assert_eq!(size(999_999_999), "1000 MB");
        assert_eq!(size(12_000_000_000), "12 GB");
    }

    #[test]
    fn times_out_of_range_show_nothing() {
        let far = Duration::from_secs(u64::MAX / 2);
        assert_eq!(modified(UNIX_EPOCH.checked_add(far).unwrap()), "");
        assert_eq!(modified(UNIX_EPOCH.checked_sub(far).unwrap()), "");
        assert_ne!(modified(UNIX_EPOCH - Duration::from_secs(1)), "");
        assert_ne!(modified(SystemTime::now()), "");
    }

    #[test]
    fn type_comes_from_the_extension() {
        let file = |name: &str| type_name(OsStr::new(name), EntryKind::File);
        assert_eq!(file("photo.JPG"), "JPEG image");
        assert_eq!(file("clip.mkv"), "MKV video");
        assert_eq!(file("data.xyz"), "XYZ file");
        assert_eq!(file("Makefile"), "File");
        assert_eq!(file(".bashrc"), "File");
        assert_eq!(
            type_name(OsStr::new("photos.jpg"), EntryKind::Folder),
            "Folder"
        );
    }
}
