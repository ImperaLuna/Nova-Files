//! The names "keep both" gives when a name is taken.
//!
//! `notes (2).txt`, `backup (2).tar.gz`, `docs (2)`: the core names copies,
//! moves and new archives this way, so a window can tell ahead which name
//! a new file will get.

use std::ffi::{OsStr, OsString};
use std::ops::RangeInclusive;
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};

const TARBALLS: [&[u8]; 4] = [b".tar.gz", b".tar.xz", b".tar.zst", b".tar.bz2"];

/// The numbers tried after the name itself, in order.
pub const NUMBERS: RangeInclusive<u32> = 2..=10_000;

/// Where a name takes its number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Naming {
    /// Before the extension, a tarball's `.tar.gz` counting as one; at the
    /// end when there is none, or the name only starts with a dot.
    File,
    /// At the end.
    Folder,
}

/// `name` with `number` in it, as `naming` places it.
pub fn numbered(name: &OsStr, number: u32, naming: Naming) -> OsString {
    let bytes = name.as_bytes();
    let split = match naming {
        Naming::Folder => bytes.len(),
        Naming::File => extension_start(bytes).unwrap_or(bytes.len()),
    };
    let (stem, extension) = bytes.split_at(split);
    let mut numbered = stem.to_vec();
    numbered.extend(format!(" ({number})").into_bytes());
    numbered.extend(extension);
    OsString::from_vec(numbered)
}

/// `name` without its extension, a tarball's `.tar.gz` counting as one,
/// as [`Naming::File`] reads it.
pub fn stem(name: &OsStr) -> &OsStr {
    let bytes = name.as_bytes();
    OsStr::from_bytes(&bytes[..extension_start(bytes).unwrap_or(bytes.len())])
}

/// The first of `name` and its numbered forms (see [`NUMBERS`]) that
/// `taken` says is free; `None` when all are taken.
pub fn first_free(
    name: &OsStr,
    naming: Naming,
    taken: impl Fn(&OsStr) -> bool,
) -> Option<OsString> {
    if !taken(name) {
        return Some(name.to_os_string());
    }
    NUMBERS
        .map(|number| numbered(name, number, naming))
        .find(|candidate| !taken(candidate))
}

fn extension_start(name: &[u8]) -> Option<usize> {
    let lower = name.to_ascii_lowercase();
    if let Some(tarball) = TARBALLS.iter().find(|tarball| lower.ends_with(tarball)) {
        let start = name.len() - tarball.len();
        if start > 0 {
            return Some(start);
        }
    }
    name.iter()
        .rposition(|&byte| byte == b'.')
        .filter(|&dot| dot > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, number: u32) -> OsString {
        numbered(OsStr::new(name), number, Naming::File)
    }

    #[test]
    fn numbers_go_before_the_extension() {
        assert_eq!(file("photo.jpg", 2), "photo (2).jpg");
        assert_eq!(file("notes", 3), "notes (3)");
        assert_eq!(file("a.b.c", 2), "a.b (2).c");
        assert_eq!(file(".bashrc", 2), ".bashrc (2)");
        assert_eq!(file(".config.toml", 2), ".config (2).toml");
        assert_eq!(file("backup.tar.gz", 2), "backup (2).tar.gz");
        assert_eq!(file("Backup.TAR.ZST", 2), "Backup (2).TAR.ZST");
        assert_eq!(file("x.tar.xz", 2), "x (2).tar.xz");
        assert_eq!(file("y.tar.bz2", 2), "y (2).tar.bz2");
        assert_eq!(file(".tar.gz", 2), ".tar (2).gz");
        assert_eq!(file("plain.tar", 2), "plain (2).tar");
        assert_eq!(
            numbered(OsStr::new("my.photos"), 2, Naming::Folder),
            "my.photos (2)"
        );
        assert_eq!(
            numbered(OsStr::from_bytes(b"caf\xe9.txt"), 2, Naming::File).as_bytes(),
            b"caf\xe9 (2).txt"
        );
    }

    #[test]
    fn a_stem_drops_the_whole_tarball_extension() {
        assert_eq!(stem(OsStr::new("backup.tar.gz")), "backup");
        assert_eq!(stem(OsStr::new("notes.txt")), "notes");
        assert_eq!(stem(OsStr::new("notes")), "notes");
        assert_eq!(stem(OsStr::new(".bashrc")), ".bashrc");
    }

    #[test]
    fn the_first_name_not_taken_is_given() {
        let taken = ["a.zip", "a (2).zip"];
        let is_taken = |name: &OsStr| taken.iter().any(|taken| name == OsStr::new(taken));
        assert_eq!(
            first_free(OsStr::new("b.zip"), Naming::File, is_taken),
            Some("b.zip".into())
        );
        assert_eq!(
            first_free(OsStr::new("a.zip"), Naming::File, is_taken),
            Some("a (3).zip".into())
        );
        assert_eq!(
            first_free(OsStr::new("a.zip"), Naming::File, |_| true),
            None
        );
    }
}
