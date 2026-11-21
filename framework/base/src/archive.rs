//! The name endings of archives, written down here and nowhere else.
//!
//! The icon, the name a new archive gets, the names that open as an
//! archive and the folder an archive unpacks into all read them from here.

use std::ffi::OsStr;

/// One of [`ENDINGS`].
pub const TAR_GZ: &str = ".tar.gz";
/// One of [`ENDINGS`].
pub const TAR_XZ: &str = ".tar.xz";
/// One of [`ENDINGS`].
pub const TAR_ZST: &str = ".tar.zst";
/// One of [`ENDINGS`].
pub const TAR_BZ2: &str = ".tar.bz2";
/// One of [`ENDINGS`].
pub const ZIP: &str = ".zip";
/// One of [`ENDINGS`].
pub const SEVEN_ZIP: &str = ".7z";
/// One of [`ENDINGS`].
pub const RAR: &str = ".rar";

/// The name endings, in lowercase, of the archives the core unpacks.
pub const ENDINGS: [&str; 13] = [
    TAR_GZ, TAR_XZ, TAR_ZST, TAR_BZ2, ".tgz", ".txz", ".tzst", ".tbz2", ".tbz", ".tar", ZIP,
    SEVEN_ZIP, RAR,
];

/// The endings in [`ENDINGS`] the core unpacks only with an installed
/// 7-Zip.
pub const SEVEN_ENDINGS: [&str; 2] = [SEVEN_ZIP, RAR];

/// The endings in [`ENDINGS`] made of two, which a name keeps whole when
/// it takes a number.
pub const TARBALLS: [&str; 4] = [TAR_GZ, TAR_XZ, TAR_ZST, TAR_BZ2];

/// The ending in [`ENDINGS`] that `name` has, in any case, when something
/// comes before it.
pub fn ending(name: &OsStr) -> Option<&'static str> {
    let name = name.as_encoded_bytes().to_ascii_lowercase();
    ENDINGS
        .into_iter()
        .find(|ending| name.len() > ending.len() && name.ends_with(ending.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_longest_ending_wins() {
        assert_eq!(ending(OsStr::new("a.tar.gz")), Some(".tar.gz"));
        assert_eq!(ending(OsStr::new("a.TAR")), Some(".tar"));
        assert_eq!(ending(OsStr::new("a.TXZ")), Some(".txz"));
        assert_eq!(ending(OsStr::new(".zip")), None);
        assert_eq!(ending(OsStr::new("dump.sql.xz")), None);
    }
}
