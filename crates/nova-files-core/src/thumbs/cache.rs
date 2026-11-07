//! The freedesktop thumbnail cache: where a file's thumbnail lives, and
//! whether the one there is still good.
//!
//! A thumbnail is named after the MD5 of its file's URI and stores the
//! file's mtime in a `Thumb::MTime` text chunk; it is good while that
//! matches. Failure markers, kept per application, follow the same rule.
//! A file cut short (it doesn't reach its end chunk) or too big to be a
//! thumbnail doesn't count, so it is made again.

use std::fs::File;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

use super::md5;

pub const NORMAL_FOLDER: &str = "normal";
pub const FAIL_FOLDER: &str = "fail/nova-files";
const EXTENSION: &str = ".png";
/// A "normal" thumbnail is a few tens of KB; anything this big isn't one.
const MAX_THUMBNAIL_BYTES: u64 = 4 * 1024 * 1024;
const MTIME_KEY: &[u8] = b"Thumb::MTime";
const PNG_SIGNATURE_LENGTH: usize = 8;
const CHUNK_LENGTH: usize = 4;
const CHUNK_TYPE: usize = 4;
const CHUNK_CRC: usize = 4;
const TEXT_TYPE: &[u8] = b"tEXt";
const END_TYPE: &[u8] = b"IEND";

/// The thumbnail's file name for the file at `uri`.
pub fn name(uri: &str) -> String {
    md5::hex(uri.as_bytes()) + EXTENSION
}

/// The thumbnail or marker `name` in `folder`, if it was made from the
/// file as it is now (`mtime`, in whole seconds).
pub fn read_if_current(folder: &Path, name: &str, mtime: u64) -> Option<Vec<u8>> {
    let png = read_capped(&folder.join(name)).ok()?;
    let stored = text(&png, MTIME_KEY)?;
    (stored == mtime.to_string().as_bytes()).then_some(png)
}

pub fn normal(thumbnails: &Path) -> PathBuf {
    thumbnails.join(NORMAL_FOLDER)
}

pub fn failed(thumbnails: &Path) -> PathBuf {
    thumbnails.join(FAIL_FOLDER)
}

/// The file's bytes; an error for one bigger than a thumbnail can be.
fn read_capped(path: &Path) -> io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_THUMBNAIL_BYTES)
        .read_to_end(&mut bytes)?;
    // A byte past the cap means the file is bigger than a thumbnail can be.
    if file.read(&mut [0])? != 0 {
        return Err(io::ErrorKind::FileTooLarge.into());
    }
    Ok(bytes)
}

/// The text of the first `tEXt` chunk named `key`, if the PNG's chunks run
/// whole to its end chunk.
fn text<'a>(png: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let mut rest = png.get(PNG_SIGNATURE_LENGTH..)?;
    let mut found = None;
    loop {
        let (length, after) = rest.split_at_checked(CHUNK_LENGTH)?;
        let length = usize::try_from(u32::from_be_bytes(length.try_into().ok()?)).ok()?;
        let (kind, after) = after.split_at_checked(CHUNK_TYPE)?;
        let (data, after) = after.split_at_checked(length)?;
        rest = after.get(CHUNK_CRC..)?;
        if kind == END_TYPE {
            return found;
        }
        if found.is_none()
            && kind == TEXT_TYPE
            && let Some((name, value)) = split_once_nul(data)
            && name == key
        {
            found = Some(value);
        }
    }
}

fn split_once_nul(data: &[u8]) -> Option<(&[u8], &[u8])> {
    let at = data.iter().position(|&byte| byte == 0)?;
    Some((&data[..at], &data[at + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG-shaped byte string: signature, then `(type, data)` chunks with
    /// dummy CRCs.
    fn png(chunks: &[(&[u8], &[u8])]) -> Vec<u8> {
        let mut out = vec![0; PNG_SIGNATURE_LENGTH];
        for (kind, data) in chunks {
            out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(data);
            out.extend_from_slice(&[0; CHUNK_CRC]);
        }
        out
    }

    #[test]
    fn finds_the_mtime_text() {
        let bytes = png(&[
            (b"IHDR", &[0; 13]),
            (b"tEXt", b"Thumb::URI\0file:///a"),
            (b"tEXt", b"Thumb::MTime\x0042"),
            (b"IEND", b""),
        ]);
        assert_eq!(text(&bytes, MTIME_KEY), Some(&b"42"[..]));
    }

    #[test]
    fn missing_or_cut_off_text_is_none() {
        let without = png(&[(b"IHDR", &[0; 13]), (b"IEND", b"")]);
        assert_eq!(text(&without, MTIME_KEY), None);
        let cut = png(&[(b"tEXt", b"Thumb::MTime\x0042")]);
        assert_eq!(text(&cut[..cut.len() - 6], MTIME_KEY), None);
        let no_end = png(&[(b"tEXt", b"Thumb::MTime\x0042"), (b"IDAT", &[0; 64])]);
        assert_eq!(text(&no_end, MTIME_KEY), None);
        let end_cut = png(&[(b"tEXt", b"Thumb::MTime\x0042"), (b"IEND", b"")]);
        assert_eq!(text(&end_cut[..end_cut.len() - 1], MTIME_KEY), None);
        assert_eq!(text(b"short", MTIME_KEY), None);
    }

    /// A thumbnail for mtime 42 that is exactly `size` bytes long.
    fn thumbnail_of_size(size: usize) -> Vec<u8> {
        let text: &[u8] = b"Thumb::MTime\x0042";
        let chunk = CHUNK_LENGTH + CHUNK_TYPE + CHUNK_CRC;
        let filler = size - PNG_SIGNATURE_LENGTH - 3 * chunk - text.len();
        let bytes = png(&[(b"tEXt", text), (b"IDAT", &vec![0; filler]), (b"IEND", b"")]);
        assert_eq!(bytes.len(), size);
        bytes
    }

    #[test]
    fn a_thumbnail_at_the_size_cap_is_read_and_a_bigger_file_is_not() {
        let folder =
            std::env::temp_dir().join(format!("nova-files-cache-cap-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let cap = usize::try_from(MAX_THUMBNAIL_BYTES).unwrap();
        let at_cap = thumbnail_of_size(cap);
        std::fs::write(folder.join("at-cap.png"), &at_cap).unwrap();
        // Whole up to the cap, so only its size can refuse it.
        let mut over = at_cap.clone();
        over.push(0);
        std::fs::write(folder.join("over.png"), &over).unwrap();

        let read_at_cap = read_if_current(&folder, "at-cap.png", 42);
        let read_over = read_if_current(&folder, "over.png", 42);
        std::fs::remove_dir_all(&folder).unwrap();

        assert_eq!(read_at_cap, Some(at_cap));
        assert_eq!(read_over, None);
    }

    #[test]
    fn markers_live_in_the_apps_own_fail_folder() {
        let thumbnails = Path::new("/cache/thumbnails");
        assert_eq!(failed(thumbnails), thumbnails.join("fail/nova-files"));
        assert_eq!(normal(thumbnails), thumbnails.join("normal"));
    }

    #[test]
    fn names_follow_the_spec() {
        assert_eq!(
            name("file:///home/jens/photos/me.png"),
            "c6ee772d9e49320e97ec29a7eb5b1697.png"
        );
    }
}
